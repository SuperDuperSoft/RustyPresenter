//! The ticker (spec §5.8): wakes every 200 ms while a timer runs, a slide waits to turn or a message waits to
//! hide, and sleeps until woken otherwise. It never bumps the live version by itself.
use crate::app::App;
use crate::automation::runtime::WAKE_MS;
use std::{sync::Arc, time::Duration};

pub async fn ticker(app: Arc<App>) {
    loop {
        let idle = app.automation_tick(chrono::Utc::now());
        if idle {
            // Woken by every timer action, show, arm and data change; the timeout keeps a missed wake harmless.
            let _ = tokio::time::timeout(Duration::from_secs(5), app.ticker_wake.notified()).await;
        } else {
            tokio::time::sleep(Duration::from_millis(WAKE_MS)).await;
        }
    }
}
