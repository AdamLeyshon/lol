use super::*;

impl PeerSvc {
    async fn prepare_replication_stream(
        selfid: NodeId,
        command_log: CommandLog,
        l: Index,
        r: Index,
    ) -> Result<LogStream> {
        let head = command_log.get_entry(l).await?;

        let st = async_stream::stream! {
            for idx in l..r {
                let x = command_log.get_entry(idx).await.unwrap();
                let e = LogStreamElem {
                    this_clock: x.this_clock,
                    command: x.command,
                };
                yield e;
            }
        };

        Ok(LogStream {
            sender_id: selfid,
            prev_clock: head.prev_clock,
            entries: Box::pin(st),
        })
    }

    pub async fn advance_replication(&self, follower_id: NodeId) -> Result<bool> {
        let peer = self.peer_contexts.read().get(&follower_id).unwrap().clone();

        let old_progress = peer.progress;
        let cur_last_log_index = self.command_log.get_log_last_index().await?;

        // More entries to send?
        let should_send = cur_last_log_index >= old_progress.next_index;
        if !should_send {
            return Ok(false);
        }

        // The entries to send may be deleted due to previous compactions.
        // In this case, replication will reset from the current snapshot index.
        let cur_snapshot_index = self.command_log.snapshot_pointer.load(Ordering::SeqCst);
        if old_progress.next_index < cur_snapshot_index {
            warn!(
                "entry not found at next_index (idx={}) for {}",
                old_progress.next_index, follower_id,
            );
            let new_progress = ReplicationProgress::new(cur_snapshot_index);
            self.peer_contexts
                .write()
                .get_mut(&follower_id)
                .unwrap()
                .progress = new_progress;
            return Ok(true);
        }

        let n_max_possible = cur_last_log_index - old_progress.next_index + 1;
        let n = std::cmp::min(old_progress.next_max_cnt, n_max_possible);
        assert!(n >= 1);

        let out_stream = Self::prepare_replication_stream(
            self.driver.selfid(),
            self.command_log.clone(),
            old_progress.next_index,
            old_progress.next_index + n,
        )
        .await?;

        let conn = self.driver.connect(follower_id.clone());
        let send_resp = conn.send_log_stream(out_stream).await;

        let new_progress = if let Ok(resp) = send_resp {
            match resp {
                response::SendLogStream { success: true, .. } => ReplicationProgress {
                    match_index: old_progress.next_index + n - 1,
                    next_index: old_progress.next_index + n,
                    next_max_cnt: n * 2,
                },
                response::SendLogStream {
                    success: false,
                    log_last_index: last_log_index,
                } => ReplicationProgress {
                    match_index: old_progress.match_index,
                    next_index: std::cmp::min(old_progress.next_index - 1, last_log_index + 1),
                    next_max_cnt: 1,
                },
            }
        } else {
            old_progress
        };

        self.peer_contexts
            .write()
            .get_mut(&follower_id)
            .unwrap()
            .progress = new_progress;

        Ok(true)
    }
}
