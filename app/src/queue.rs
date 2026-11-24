use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;

use crate::download::{AudioFormat, DownloadError, Downloader, Update};

pub enum State {
    Waiting,
    Downloading(f32),
    /// The download is complete and FFmpeg is writing the audio file.
    Converting,
    Saved(PathBuf),
    Failed(DownloadError),
}

pub struct Job {
    pub url: String,
    pub title: Option<String>,
    pub state: State,
}

enum Event {
    Started(usize),
    Updated(usize, Update),
    Finished(usize, Result<PathBuf, DownloadError>),
}

#[derive(Default)]
pub struct Queue {
    jobs: Vec<Job>,
    receiver: Option<Receiver<Event>>,
}

impl Queue {
    pub fn jobs(&self) -> &[Job] {
        &self.jobs
    }

    pub fn is_running(&self) -> bool {
        self.receiver.is_some()
    }

    /// Replaces the current jobs and downloads `urls` in order on a worker thread.
    /// The worker calls `notify` whenever `poll` has something new to read.
    pub fn start(
        &mut self,
        downloader: impl Downloader + Send + 'static,
        urls: Vec<String>,
        format: AudioFormat,
        folder: PathBuf,
        notify: impl Fn() + Send + 'static,
    ) {
        self.jobs = urls
            .iter()
            .map(|url| Job {
                url: url.clone(),
                title: None,
                state: State::Waiting,
            })
            .collect();
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        thread::spawn(move || {
            let send = |event| {
                let _ = sender.send(event);
                notify();
            };
            for (index, url) in urls.iter().enumerate() {
                send(Event::Started(index));
                let result = downloader.download(url, format, &folder, &mut |update| {
                    send(Event::Updated(index, update))
                });
                send(Event::Finished(index, result));
            }
            drop(sender);
            notify();
        });
    }

    /// Applies every pending worker event without blocking.
    pub fn poll(&mut self) {
        let Some(receiver) = &self.receiver else {
            return;
        };
        loop {
            match receiver.try_recv() {
                Ok(Event::Started(index)) => self.jobs[index].state = State::Downloading(0.0),
                Ok(Event::Updated(index, Update::Title(title))) => {
                    self.jobs[index].title = Some(title)
                }
                Ok(Event::Updated(index, Update::Progress(progress))) => {
                    self.jobs[index].state = if progress >= 1.0 {
                        State::Converting
                    } else {
                        State::Downloading(progress)
                    };
                }
                Ok(Event::Finished(index, result)) => {
                    self.jobs[index].state = match result {
                        Ok(file) => State::Saved(file),
                        Err(error) => State::Failed(error),
                    };
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.receiver = None;
                    for job in &mut self.jobs {
                        if matches!(
                            job.state,
                            State::Waiting | State::Downloading(_) | State::Converting
                        ) {
                            job.state = State::Failed(DownloadError::WorkerStopped);
                        }
                    }
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use super::*;

    struct Fake<F>(F);

    impl<F> Downloader for Fake<F>
    where
        F: Fn(&str, &mut dyn FnMut(Update)) -> Result<PathBuf, DownloadError>,
    {
        fn download(
            &self,
            url: &str,
            _format: AudioFormat,
            _folder: &Path,
            progress: &mut dyn FnMut(Update),
        ) -> Result<PathBuf, DownloadError> {
            (self.0)(url, progress)
        }
    }

    fn start<F>(urls: &[&str], download: F) -> Queue
    where
        F: Fn(&str, &mut dyn FnMut(Update)) -> Result<PathBuf, DownloadError> + Send + 'static,
    {
        let mut queue = Queue::default();
        let urls = urls.iter().map(|url| (*url).to_owned()).collect();
        queue.start(
            Fake(download),
            urls,
            AudioFormat::Mp3,
            PathBuf::from("out"),
            || {},
        );
        queue
    }

    fn poll_until(queue: &mut Queue, done: impl Fn(&Queue) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done(queue) {
            assert!(Instant::now() < deadline, "the queue did not settle");
            thread::sleep(Duration::from_millis(1));
            queue.poll();
        }
    }

    fn finish(queue: &mut Queue) {
        poll_until(queue, |queue| !queue.is_running());
    }

    #[test]
    fn new_queue_is_idle_and_empty() {
        let mut queue = Queue::default();
        queue.poll();
        assert!(!queue.is_running());
        assert!(queue.jobs().is_empty());
    }

    #[test]
    fn saves_every_job_in_order() {
        let order = Arc::new(Mutex::new(Vec::new()));
        let seen = order.clone();
        let mut queue = start(&["a", "b", "c"], move |url, progress| {
            seen.lock().unwrap().push(url.to_owned());
            progress(Update::Title(format!("Title {url}")));
            progress(Update::Progress(1.0));
            Ok(PathBuf::from(format!("{url}.mp3")))
        });
        assert!(queue.is_running());
        finish(&mut queue);

        assert_eq!(*order.lock().unwrap(), ["a", "b", "c"]);
        for (job, url) in queue.jobs().iter().zip(["a", "b", "c"]) {
            assert_eq!(job.url, url);
            assert_eq!(job.title, Some(format!("Title {url}")));
            assert!(
                matches!(&job.state, State::Saved(file) if *file == Path::new(&format!("{url}.mp3")))
            );
        }
    }

    #[test]
    fn failed_job_does_not_stop_the_next_one() {
        let mut queue = start(&["bad", "good"], |url, _| match url {
            "bad" => Err(DownloadError::Reported("Video unavailable".to_owned())),
            _ => Ok(PathBuf::from("good.mp3")),
        });
        finish(&mut queue);

        assert!(
            matches!(&queue.jobs()[0].state, State::Failed(DownloadError::Reported(error)) if error == "Video unavailable")
        );
        assert!(matches!(queue.jobs()[1].state, State::Saved(_)));
    }

    #[test]
    fn worker_crash_fails_unfinished_jobs_and_keeps_saved_ones() {
        let mut queue = start(&["good", "crash", "never"], |url, _| match url {
            "good" => Ok(PathBuf::from("good.mp3")),
            _ => panic!("expected by this test"),
        });
        finish(&mut queue);

        assert!(matches!(queue.jobs()[0].state, State::Saved(_)));
        for job in &queue.jobs()[1..] {
            assert!(matches!(
                job.state,
                State::Failed(DownloadError::WorkerStopped)
            ));
        }
    }

    #[test]
    fn job_moves_from_downloading_to_converting_to_saved() {
        let (step, blocked) = mpsc::channel::<()>();
        let mut queue = start(&["a"], move |_, progress| {
            progress(Update::Progress(0.999));
            let _ = blocked.recv();
            progress(Update::Progress(1.0));
            let _ = blocked.recv();
            Ok(PathBuf::from("saved.mp3"))
        });
        poll_until(
            &mut queue,
            |queue| matches!(queue.jobs()[0].state, State::Downloading(value) if value == 0.999),
        );
        step.send(()).unwrap();
        poll_until(&mut queue, |queue| {
            matches!(queue.jobs()[0].state, State::Converting)
        });
        assert!(queue.is_running());
        step.send(()).unwrap();
        finish(&mut queue);
        assert!(matches!(queue.jobs()[0].state, State::Saved(_)));
    }

    #[test]
    fn worker_crash_while_converting_fails_the_job() {
        let mut queue = start(&["a"], |_, progress| {
            progress(Update::Progress(1.0));
            panic!("expected by this test")
        });
        finish(&mut queue);
        assert!(matches!(
            queue.jobs()[0].state,
            State::Failed(DownloadError::WorkerStopped)
        ));
    }

    #[test]
    fn reports_progress_while_the_next_job_waits() {
        let (release, blocked) = mpsc::channel::<()>();
        let mut queue = start(&["first", "second"], move |_, progress| {
            progress(Update::Progress(0.5));
            let _ = blocked.recv();
            Ok(PathBuf::from("saved.mp3"))
        });
        poll_until(
            &mut queue,
            |queue| matches!(queue.jobs()[0].state, State::Downloading(value) if value == 0.5),
        );
        assert!(queue.is_running());
        assert!(matches!(queue.jobs()[1].state, State::Waiting));

        drop(release);
        finish(&mut queue);
        assert!(matches!(queue.jobs()[1].state, State::Saved(_)));
    }

    #[test]
    fn passes_format_and_folder_and_notifies_for_each_event() {
        struct Recorder(Arc<Mutex<Option<(AudioFormat, PathBuf)>>>);
        impl Downloader for Recorder {
            fn download(
                &self,
                _url: &str,
                format: AudioFormat,
                folder: &Path,
                progress: &mut dyn FnMut(Update),
            ) -> Result<PathBuf, DownloadError> {
                *self.0.lock().unwrap() = Some((format, folder.to_owned()));
                progress(Update::Progress(0.5));
                Ok(PathBuf::from("saved.flac"))
            }
        }
        let received = Arc::new(Mutex::new(None));
        let notified = Arc::new(AtomicUsize::new(0));
        let counter = notified.clone();
        let mut queue = Queue::default();
        queue.start(
            Recorder(received.clone()),
            vec!["a".to_owned()],
            AudioFormat::Flac,
            PathBuf::from("music"),
            move || {
                counter.fetch_add(1, Ordering::SeqCst);
            },
        );
        finish(&mut queue);

        assert_eq!(
            *received.lock().unwrap(),
            Some((AudioFormat::Flac, PathBuf::from("music")))
        );
        // Started, progress, finished, and one call after the channel closes.
        let deadline = Instant::now() + Duration::from_secs(10);
        while notified.load(Ordering::SeqCst) < 4 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(notified.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn starting_again_replaces_the_previous_jobs() {
        let mut queue = start(&["a", "b"], |_, _| Ok(PathBuf::from("saved.mp3")));
        finish(&mut queue);
        queue.start(
            Fake(|_: &str, _: &mut dyn FnMut(Update)| Err(DownloadError::NoSavedFile)),
            vec!["c".to_owned()],
            AudioFormat::Mp3,
            PathBuf::from("out"),
            || {},
        );
        finish(&mut queue);

        assert_eq!(queue.jobs().len(), 1);
        assert_eq!(queue.jobs()[0].url, "c");
    }
}
