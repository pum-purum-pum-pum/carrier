* tests
* * test follow method
* * test incorrect message content
* * test incorrect user id



* timeouts \/
* delay (for event source to finish events)
* logs
* what is fuse? \/
* remove UserId \/
* error handling. Where cut? \/
* shutdown?
* cancelation ?
* make sure that we processing all queue? \/

* use users from their lib -- not possible

* test client doesn't close the connection because I'm keeping peers open \/

* In process_event_source if we not close the socket before blocking on "process sequence" at the end everything is freezed.

* we locked sequenced queue forever in process queue? \No/

* timeout by 8-sized batches

* assumption that solution is slower then test locally is not correct
* unclear task about "silentry ignored"
* mistake in `handle user updates` counter = 0 should be counter = 1 (no events have 0 id)
* why unfollow "expect"
* unfollow is wrong?????
    /// Target should be removed from Actor's followers. No one is expected to receive this event.
    pub fn unfollow(&mut self, from: u32, to: u32) {


                    // not that chat-app crate is using threads so:
                    // SERVER_SHUTDOWN_TIME = timeout * NUMBER_OF_CONNECTIONS / NUMBER_OF_THREADS


Assumptions about the task:
	* private functions of the provided library should not be used
	* code of the provided provided library should not be changed (expose helpful functions, update dependencies etc.)

Random notes:
	* Test server doesn't check the case when my program is not sending any messages at all.
<!-- Panic while holding a Mutex(checking expected msg) => lock return Error and panics in Mutex -->





Есть некоторое хранилище куда я созряняю сообщения, которые не могу отправить сразу. 
Код примерно такой

читать сообщения
	пихнуть сообщения в структурку
	отослать сообщения из структурки

структурка Rc<Mutex<Data>>






	    let mut peer = Peer::new(state, stream).await?;
        // if queue.lock().await.empty() {
        //     info!("empty");
        //     break
        // }
    #[derive(Debug)]
    enum GracefulMessage {
        Queue(bool),
        Message(Option<String>),
    }
    'message_loop: loop {
        let mut queue = Box::pin(queue.lock().fuse());
        let mut msg = peer.rx.next().fuse();
        info!("selecting");
        // TODO timeout
        // we want to close connection if there is no more messages in event source
        let message = select!(
            queue = queue => {
                GracefulMessage::Queue(queue.empty())
            }
            msg = msg => {
                GracefulMessage::Message(msg)
            }
            complete => break 'message_loop
        );
        info!("{:?}", message);
        match message {
            GracefulMessage::Queue(empty) => {
                info!("{:?}", empty);
                if empty {
                    break 'message_loop
                }
            }
            GracefulMessage::Message(Some(msg)) => {
                peer.lines.send(msg).await?;
                info!("sended");
            }
            GracefulMessage::Message(None) => {
                break 'message_loop
            }
        }
    }
    info!("process client done");
    Ok(())
