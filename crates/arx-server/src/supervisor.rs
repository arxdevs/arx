use futures::FutureExt;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::time::Duration;

const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(60);

pub fn spawn_supervised<F, Fut>(name: &'static str, mut factory: F)
where
    F: FnMut() -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        let mut backoff = INITIAL_BACKOFF;
        loop {
            let outcome = AssertUnwindSafe(factory()).catch_unwind().await;
            match outcome {
                Ok(()) => {
                    tracing::warn!(task = name, "supervised task exited, restarting");
                }
                Err(panic) => {
                    let msg = panic_message(&panic);
                    tracing::error!(task = name, panic = %msg, "supervised task panicked, restarting");
                }
            }
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(MAX_BACKOFF);
        }
    });
}

fn panic_message(p: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = p.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic>".to_string()
    }
}
