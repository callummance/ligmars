use ligmars::{
    client::Client,
    host::{Host, LGMPQueueConfig},
};
use shared_memory::Shmem;

const SHMEM_SIZE: usize = 10 * (2 << 20); //10MiB

fn create_shmem() -> Shmem {
    shared_memory::ShmemConf::new()
        .size(SHMEM_SIZE)
        .create()
        .expect("Unable to create shared memory region")
}

fn new_handle(host_shmem: &Shmem) -> Shmem {
    let shmem_os_id = host_shmem.get_os_id();
    shared_memory::ShmemConf::new()
        .os_id(shmem_os_id)
        .open()
        .expect("Failed to create addition handle to SHMEM region")
}

#[allow(clippy::vec_box)]
fn allocate_handles(num_clients: usize) -> (Box<Shmem>, Vec<Box<Shmem>>) {
    let host_shmem = create_shmem();

    let clients_vec: Vec<Box<Shmem>> = (0..num_clients)
        .map(|_| new_handle(&host_shmem))
        .map(Box::new)
        .collect();
    let boxed_host = Box::new(host_shmem);

    (boxed_host, clients_vec)
}

pub fn allocate_host_and_clients(num_clients: usize, udata: &[u8]) -> (Host, Vec<Client>) {
    let (host_handle, client_handles) = allocate_handles(num_clients);
    let host = Host::init(host_handle, udata).expect("Failed to init host struct");
    let clients = client_handles
        .into_iter()
        .map(|handle| Client::init(handle).expect("Failed to init client struct"))
        .collect();

    (host, clients)
}

pub const DEFAULT_QUEUE_CONF: LGMPQueueConfig = LGMPQueueConfig {
    queue_id: 0,
    num_messages: 10,
    sub_timeout: 1000,
};
