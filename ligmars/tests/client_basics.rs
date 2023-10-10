mod common;

#[test]
fn client_basic_functionality() {
    //Data to be used in the test
    let udata = vec![0x69_u8; 32];
    let test_string = "This is a test message.".to_string();

    let (mut host, mut clients) = common::allocate_host_and_clients(1, &udata);

    //Setup host and queue
    let host_queue = host
        .queue_new(common::DEFAULT_QUEUE_CONF)
        .expect("Failed to create queue");
    let shared_allocation = host
        .mem_alloc(1024)
        .expect("Failed to allocate shared memory");
    shared_allocation
        .lock()
        .unwrap()
        .copy_from_bytes(test_string.as_bytes())
        .expect("Failed to send message from host");

    //Check clients are not yet running a valid session
    clients.iter().for_each(|client| {
        assert!(
            !client.client_session_valid(),
            "Client reported a valid session despite one not being initialised"
        )
    });

    //Wait a bit then run process on the host to prevent timestamp from tripping checks
    std::thread::sleep(std::time::Duration::from_millis(10));
    host.process().expect("Failed to run process on host");

    //Init session on client
    clients.iter_mut().for_each(|client| {
        let (recv_udata, _id) = client
            .client_session_init()
            .expect("session_init call on client failed");

        assert_eq!(
            recv_udata, udata,
            "data recieved by client init ({:?}) did not match data provided to host({:?})",
            recv_udata, udata
        );
    });

    //Check clients now report a valid session
    clients.iter().for_each(|client| {
        assert!(
            client.client_session_valid(),
            "Client reported that a valid session did not exist"
        )
    });

    //Subscribe on client
    let mut client_queues: Vec<ligmars::client::ClientQueueHandle> = clients
        .iter_mut()
        .map(|client| {
            client
                .client_subscribe(common::DEFAULT_QUEUE_CONF.queue_id)
                .expect("Failed to subscribe to channel from client")
        })
        .collect();

    //Check host serial is 0
    client_queues.iter_mut().for_each(|q| {
        assert_eq!(
            q.get_serial().unwrap(),
            0,
            "Client returned nonzero read serial"
        )
    });

    //Check that subscribe has succeeded
    assert!(host_queue.has_subs());

    //Unsubscribe client and check it succeeded
    drop(client_queues);
    assert!(!host_queue.has_subs());
}
