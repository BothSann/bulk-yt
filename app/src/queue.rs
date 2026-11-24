use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;

use crate::cancel::Cancel;
use crate::downloader::{DownloadError, Downloader, Update};
use crate::format::AudioFormat;

/// The number of downloads that can run at once.
const MAX_WORKERS: usize = 3;

pub enum State {
    Waiting,
    Downloading(f32),
    /// The download is complete and FFmpeg is writing the audio file.
    Converting,
    Saved(PathBuf),
    Failed(DownloadError),
    Cancelled,
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
    cancel: Cancel,
}

/// One worker's side of the channel. Dropping it closes that side and then wakes the UI, also when
/// the worker panics. The last one to drop closes the channel before it wakes the UI.
struct Reporter<N: Fn()> {
    sender: Option<Sender<Event>>,
    notify: Arc<N>,
}

impl<N: Fn()> Reporter<N> {
    fn send(&self, event: Event) {
        if let Some(sender) = &self.sender {
            let _ = sender.send(event);
        }
        (self.notify)();
    }
}

impl<N: Fn()> Drop for Reporter<N> {
    fn drop(&mut self) {
        self.sender = None;
        (self.notify)();
    }
}

/// The work that the workers of one batch share.
struct Batch<D> {
    downloader: D,
    urls: Vec<String>,
    format: AudioFormat,
    folder: PathBuf,
    cancel: Cancel,
    /// The next job to start.
    next: AtomicUsize,
}

impl<D: Downloader> Batch<D> {
    /// Takes jobs in order until none are left.
    fn work(&self, reporter: &Reporter<impl Fn()>) {
        loop {
            let index = self.next.fetch_add(1, Ordering::SeqCst);
            let Some(url) = self.urls.get(index) else {
                return;
            };
            if self.cancel.is_requested() {
                reporter.send(Event::Finished(index, Err(DownloadError::Cancelled)));
                continue;
            }
            reporter.send(Event::Started(index));
            let download = || {
                self.downloader.download(
                    url,
                    self.format,
                    &self.folder,
                    &self.cancel,
                    &mut |update| reporter.send(Event::Updated(index, update)),
                )
            };
            // A crash fails its own job, and this worker takes the next one.
            let result = panic::catch_unwind(AssertUnwindSafe(download))
                .unwrap_or(Err(DownloadError::WorkerStopped));
            reporter.send(Event::Finished(index, result));
        }
    }
}

impl Queue {
    pub fn jobs(&self) -> &[Job] {
        &self.jobs
    }

    pub fn is_running(&self) -> bool {
        self.receiver.is_some()
    }

    pub fn is_cancelling(&self) -> bool {
        self.is_running() && self.cancel.is_requested()
    }

    /// Stops the running downloads and skips the waiting ones. Does not block.
    pub fn cancel(&self) {
        if self.is_running() {
            self.cancel.request();
        }
    }

    /// Replaces the current jobs and downloads `urls` on up to `MAX_WORKERS` threads. Jobs start in
    /// order. The workers call `notify` whenever `poll` has something new to read.
    pub fn start(
        &mut self,
        downloader: impl Downloader + Send + Sync + 'static,
        urls: Vec<String>,
        format: AudioFormat,
        folder: PathBuf,
        notify: impl Fn() + Send + Sync + 'static,
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
        self.cancel = Cancel::default();
        let workers = urls.len().clamp(1, MAX_WORKERS);
        let batch = Arc::new(Batch {
            downloader,
            urls,
            format,
            folder,
            cancel: self.cancel.clone(),
            next: AtomicUsize::new(0),
        });
        let notify = Arc::new(notify);
        for _ in 0..workers {
            let batch = batch.clone();
            let reporter = Reporter {
                sender: Some(sender.clone()),
                notify: notify.clone(),
            };
            thread::spawn(move || batch.work(&reporter));
        }
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
                        Err(DownloadError::Cancelled) => State::Cancelled,
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
    use std::sync::Mutex;
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
            _cancel: &Cancel,
            progress: &mut dyn FnMut(Update),
        ) -> Result<PathBuf, DownloadError> {
            (self.0)(url, progress)
        }
    }

    fn start<F>(urls: &[&str], download: F) -> Queue
    where
        F: Fn(&str, &mut dyn FnMut(Update)) -> Result<PathBuf, DownloadError>
            + Send
            + Sync
            + 'static,
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
    fn saves_every_job_and_keeps_their_order() {
        let mut queue = start(&["a", "b", "c"], |url, progress| {
            progress(Update::Title(format!("Title {url}")));
            progress(Update::Progress(1.0));
            Ok(PathBuf::from(format!("{url}.mp3")))
        });
        assert!(queue.is_running());
        finish(&mut queue);

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
    fn crashes_fail_their_own_jobs_and_the_rest_still_run() {
        // More crashes than workers, so a worker that ends after a crash leaves jobs behind.
        let urls = ["crash", "crash", "crash", "crash", "good", "good"];
        let mut queue = start(&urls, |url, _| match url {
            "good" => Ok(PathBuf::from("good.mp3")),
            _ => panic!("expected by this test"),
        });
        finish(&mut queue);

        for job in &queue.jobs()[..4] {
            assert!(matches!(
                job.state,
                State::Failed(DownloadError::WorkerStopped)
            ));
        }
        for job in &queue.jobs()[4..] {
            assert!(matches!(job.state, State::Saved(_)));
        }
    }

    #[test]
    fn crash_fails_its_job_while_the_others_run() {
        let (release, blocked) = mpsc::channel::<()>();
        let blocked = Mutex::new(blocked);
        let mut queue = start(&["slow", "crash"], move |url, _| {
            if url == "crash" {
                panic!("expected by this test");
            }
            let _ = blocked.lock().unwrap().recv();
            Ok(PathBuf::from("slow.mp3"))
        });
        poll_until(&mut queue, |queue| {
            matches!(
                queue.jobs()[1].state,
                State::Failed(DownloadError::WorkerStopped)
            )
        });
        assert!(queue.is_running());
        drop(release);
        finish(&mut queue);
        assert!(matches!(queue.jobs()[0].state, State::Saved(_)));
    }

    #[test]
    fn runs_three_downloads_at_once_and_starts_the_rest_in_order() {
        #[derive(Default)]
        struct Log {
            started: Vec<String>,
            running: usize,
            most: usize,
        }
        let log = Arc::new(Mutex::new(Log::default()));
        let (release, blocked) = mpsc::channel::<()>();
        let blocked = Mutex::new(blocked);
        let urls = ["a", "b", "c", "d", "e", "f", "g"];
        let mut queue = start(&urls, {
            let log = log.clone();
            move |url, _| {
                {
                    let mut log = log.lock().unwrap();
                    log.started.push(url.to_owned());
                    log.running += 1;
                    log.most = log.most.max(log.running);
                }
                let _ = blocked.lock().unwrap().recv();
                log.lock().unwrap().running -= 1;
                Ok(PathBuf::from("saved.mp3"))
            }
        });
        // Each release lets one download end, so exactly one more can start.
        for started in 3..=urls.len() {
            poll_until(&mut queue, |_| log.lock().unwrap().started.len() >= started);
            // Time for a download over the limit to start.
            thread::sleep(Duration::from_millis(50));
            queue.poll();
            let log = log.lock().unwrap();
            assert_eq!(log.started.len(), started);
            assert_eq!(log.running, 3);
            for job in &queue.jobs()[started..] {
                assert!(matches!(job.state, State::Waiting), "{}", job.url);
            }
            drop(log);
            release.send(()).unwrap();
        }
        drop(release);
        finish(&mut queue);

        let mut log = log.lock().unwrap();
        assert_eq!(log.most, 3);
        // The first three start together, so they can reach the downloader in any order.
        log.started[..3].sort();
        assert_eq!(log.started, urls);
        for job in queue.jobs() {
            assert!(matches!(job.state, State::Saved(_)), "{}", job.url);
        }
    }

    #[test]
    fn job_moves_from_downloading_to_converting_to_saved() {
        let (step, blocked) = mpsc::channel::<()>();
        let blocked = Mutex::new(blocked);
        let mut queue = start(&["a"], move |_, progress| {
            progress(Update::Progress(0.999));
            let _ = blocked.lock().unwrap().recv();
            progress(Update::Progress(1.0));
            let _ = blocked.lock().unwrap().recv();
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
        let blocked = Mutex::new(blocked);
        let mut queue = start(&["a", "b", "c", "next"], move |_, progress| {
            progress(Update::Progress(0.5));
            let _ = blocked.lock().unwrap().recv();
            Ok(PathBuf::from("saved.mp3"))
        });
        poll_until(&mut queue, |queue| {
            queue.jobs()[..3]
                .iter()
                .all(|job| matches!(job.state, State::Downloading(value) if value == 0.5))
        });
        assert!(queue.is_running());
        assert!(matches!(queue.jobs()[3].state, State::Waiting));

        drop(release);
        finish(&mut queue);
        assert!(matches!(queue.jobs()[3].state, State::Saved(_)));
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
                _cancel: &Cancel,
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

    /// Saves "good", fails "bad", and runs anything else until it is cancelled.
    struct UntilCancelled(Arc<Mutex<Vec<String>>>);

    impl Downloader for UntilCancelled {
        fn download(
            &self,
            url: &str,
            _format: AudioFormat,
            _folder: &Path,
            cancel: &Cancel,
            progress: &mut dyn FnMut(Update),
        ) -> Result<PathBuf, DownloadError> {
            self.0.lock().unwrap().push(url.to_owned());
            match url {
                "good" => Ok(PathBuf::from("good.mp3")),
                "bad" => Err(DownloadError::NoSavedFile),
                _ => {
                    progress(Update::Progress(0.5));
                    let deadline = Instant::now() + Duration::from_secs(10);
                    while !cancel.is_requested() && Instant::now() < deadline {
                        thread::sleep(Duration::from_millis(1));
                    }
                    Err(DownloadError::Cancelled)
                }
            }
        }
    }

    fn start_until_cancelled(queue: &mut Queue, urls: &[&str]) -> Arc<Mutex<Vec<String>>> {
        let started = Arc::new(Mutex::new(Vec::new()));
        queue.start(
            UntilCancelled(started.clone()),
            urls.iter().map(|url| (*url).to_owned()).collect(),
            AudioFormat::Mp3,
            PathBuf::from("out"),
            || {},
        );
        started
    }

    #[test]
    fn cancel_keeps_finished_jobs_and_cancels_the_rest() {
        let mut queue = Queue::default();
        let urls = ["good", "bad", "slow 1", "slow 2", "slow 3", "next", "last"];
        let started = start_until_cancelled(&mut queue, &urls);
        poll_until(&mut queue, |queue| {
            queue.jobs()[2..5]
                .iter()
                .all(|job| matches!(job.state, State::Downloading(value) if value == 0.5))
        });
        assert!(!queue.is_cancelling());
        queue.cancel();
        assert!(queue.is_cancelling());
        finish(&mut queue);

        assert!(!queue.is_cancelling());
        let jobs = queue.jobs();
        assert!(matches!(jobs[0].state, State::Saved(_)));
        assert!(matches!(
            jobs[1].state,
            State::Failed(DownloadError::NoSavedFile)
        ));
        for job in &jobs[2..] {
            assert!(matches!(job.state, State::Cancelled), "{}", job.url);
        }
        let mut started = started.lock().unwrap();
        started.sort();
        assert_eq!(*started, ["bad", "good", "slow 1", "slow 2", "slow 3"]);
    }

    #[test]
    fn new_batch_runs_after_a_cancelled_one() {
        let mut queue = Queue::default();
        start_until_cancelled(&mut queue, &["slow"]);
        queue.cancel();
        finish(&mut queue);
        // A request with nothing running must not affect the next batch.
        queue.cancel();

        start_until_cancelled(&mut queue, &["good"]);
        finish(&mut queue);
        assert!(matches!(queue.jobs()[0].state, State::Saved(_)));
    }

    #[test]
    fn crash_fails_the_job_closes_the_channel_and_then_notifies() {
        let notified = Arc::new(AtomicUsize::new(0));
        let counter = notified.clone();
        let mut queue = Queue::default();
        queue.start(
            Fake(
                |_: &str, _: &mut dyn FnMut(Update)| -> Result<PathBuf, DownloadError> {
                    panic!("expected by this test")
                },
            ),
            vec!["a".to_owned()],
            AudioFormat::Mp3,
            PathBuf::from("out"),
            move || {
                counter.fetch_add(1, Ordering::SeqCst);
            },
        );
        // One call for the started job, one for its failure, and one for the closed channel.
        let deadline = Instant::now() + Duration::from_secs(10);
        while notified.load(Ordering::SeqCst) < 3 {
            assert!(Instant::now() < deadline, "the panic did not notify");
            thread::sleep(Duration::from_millis(1));
        }
        queue.poll();
        assert!(!queue.is_running());
        assert!(matches!(
            queue.jobs()[0].state,
            State::Failed(DownloadError::WorkerStopped)
        ));
    }
}
