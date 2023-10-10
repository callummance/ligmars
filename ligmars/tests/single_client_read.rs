mod common;

#[test]
fn single_client_read() {
    //Data to be used in the test
    let udata = vec![0x69_u8; 32];
    let test_string = "This is a test message.".to_string();

    let (mut host, mut clients) = common::allocate_host_and_clients(1, &udata);

    //Setup host and queue
    let mut host_queue = host
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
    //Subscribe on client
    let mut client_queues: Vec<ligmars::client::ClientQueueHandle> = clients
        .iter_mut()
        .map(|client| {
            client
                .client_subscribe(common::DEFAULT_QUEUE_CONF.queue_id)
                .expect("Failed to subscribe to channel from client")
        })
        .collect();

    //Send message from host
    host_queue
        .post_shared_mem(0, shared_allocation.lock().unwrap())
        .expect("Failed to send message from host");

    //Recieve from client
    let c_queue = &mut client_queues[0];
    let msg = c_queue
        .process()
        .expect("Failed to retrieve message on client");
    let msg_cstr =
        std::ffi::CStr::from_bytes_until_nul(&msg.mem).expect("Failed to decode message string");
    let msg_str = msg_cstr.to_string_lossy().into_owned();

    assert_eq!(
        test_string.as_str(),
        msg_str,
        "Sent and recieved messages did not match"
    )
}
