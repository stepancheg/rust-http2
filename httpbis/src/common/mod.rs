//! Common code for client and server

pub(crate) mod atomic_box_option;
pub(crate) mod client_or_server;
pub(crate) mod closed_streams;
pub(crate) mod conf;
pub(crate) mod conn;
pub(crate) mod conn_read;
pub(crate) mod conn_write;
pub(crate) mod hash_set_shallow_clone;
pub(crate) mod increase_in_window;
pub(crate) mod init_where;
pub(crate) mod loop_event;
pub(crate) mod pump_stream_to_write_loop;
pub(crate) mod sender;
pub(crate) mod stream;
pub(crate) mod stream_from_network;
pub(crate) mod stream_handler;
pub(crate) mod stream_map;
pub(crate) mod stream_queue;
pub(crate) mod stream_queue_sync;
pub(crate) mod types;
pub(crate) mod waiters;
pub(crate) mod window_size;
