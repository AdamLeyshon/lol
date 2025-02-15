use super::*;

#[derive(Clone)]
pub struct Thread {
    command_log: CommandLog,
}
impl Thread {
    async fn run_once(&self) -> Result<()> {
        self.command_log.delete_old_snapshots().await
    }

    fn do_loop(self) -> ThreadHandle {
        let hdl = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(100));
            loop {
                interval.tick().await;
                let fut = {
                    let this = self.clone();
                    async move { this.run_once().await }
                };
                let _ = defensive_panic_guard(fut).await;
            }
        })
        .abort_handle();

        ThreadHandle(hdl)
    }
}

pub fn new(command_log: CommandLog) -> ThreadHandle {
    Thread { command_log }.do_loop()
}
