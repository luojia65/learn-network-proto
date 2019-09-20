use std::sync::mpsc;

pub enum Message {
    Data(Vec<u8>, u8),
    Ack,
    Nack,
    SimulationEnd,
}

pub mod transport_layer {
    use std::sync::mpsc;
    use super::Message;

    #[derive(Debug)]
    pub enum Error {
        MpscError
    }

    pub type Result<T> = core::result::Result<T, Error>;

    pub struct Sender {
        output: mpsc::Sender<Message>,
        input: mpsc::Receiver<Message>,
    }

    pub struct Receiver {
        input: mpsc::Receiver<Message>,
        output: mpsc::Sender<Message>,
    }

    impl Sender {
        pub fn new(
            output: mpsc::Sender<Message>,
            input: mpsc::Receiver<Message>,
        ) -> Self {
            Sender { output, input }
        }

        pub fn send(&self, buf: &[u8]) -> Result<usize> {
            let len = buf.len();
            let mut sum = 0;
            for byte in buf {
                sum = u8::overflowing_add(sum, *byte).0;
            }
            self.output.send(Message::Data(buf.to_owned(), sum))
                .or_else(|_| return Err(Error::MpscError))?;
            Ok(len)
        }

        pub fn simulation_end(&self) {
            self.output.send(Message::SimulationEnd).unwrap();
        }
    }

    impl Receiver {
        pub fn new(
            input: mpsc::Receiver<Message>,
            output: mpsc::Sender<Message>,
        ) -> Self {
            Receiver { input, output }
        }

        pub fn recv(&self, buf: &mut [u8]) -> Result<usize> {
            let len = match self.input.recv() {
                Ok(Message::Data(src, _sum)) => {
                    let len = usize::min(src.len(), buf.len());
                    for i in 0..len {
                        buf[i] = src[i]
                    };
                    len
                },
                Ok(_) => 0,
                Err(_) => return Err(Error::MpscError)
            };
            Ok(len)
        }
    }
}

pub mod network_layer {
    use std::sync::mpsc;
    use super::Message;

    pub struct BitError {
        send_in: mpsc::Receiver<Message>, 
        recv_out: mpsc::Sender<Message>,
        send_out: mpsc::Sender<Message>,
        recv_in: mpsc::Receiver<Message>, 
        possibility: f64,
    }

    impl BitError {
        pub fn new(
            send_in: mpsc::Receiver<Message>, 
            recv_out: mpsc::Sender<Message>,
            send_out: mpsc::Sender<Message>,
            recv_in: mpsc::Receiver<Message>, 
            possibility: f64
        ) -> Self {
            BitError { send_in, recv_out, send_out, recv_in, possibility }
        }

        pub fn run(&mut self) {
            loop {
                let should_break = self.process();
                if should_break {
                    return;
                }
            }
        } 

        fn process(&mut self) -> bool {
            use rand::Rng;
            let mut rng = rand::thread_rng();
            match self.send_in.try_recv() {
                Ok(Message::Data(mut msg, mut sum)) => {
                    if rng.gen_bool(self.possibility) {
                        let index = rng.gen_range(0, msg.len() + 1);
                        let bit_index = rng.gen_range(0, 8);
                        let mask = !(1 << bit_index); 
                        if index == msg.len() {
                            sum ^= mask;
                        } else {
                            msg[index] ^= mask;
                        }
                    }
                    self.recv_out.send(Message::Data(msg, sum)).unwrap()
                },
                Ok(Message::SimulationEnd) => return true,
                Ok(_) => {},
                Err(mpsc::TryRecvError::Empty) => {},
                Err(_) => return false,
            }
            match self.recv_in.try_recv() {
                Ok(Message::Data(_, _)) => {}
                Ok(m) => self.send_out.send(m).unwrap(),
                Err(mpsc::TryRecvError::Empty) => {},
                Err(_) => return false,
            }
            return false;
        }

    }
}

use std::thread;
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

fn main() {
    let (tx_send, rx_in) = mpsc::channel();
    let (tx_in, rx_send) = mpsc::channel();
    let (tx_recv, rx_out) = mpsc::channel();
    let (tx_out, rx_recv) = mpsc::channel();
    let mut reliable = network_layer::BitError::new(rx_in, tx_out, tx_in, rx_out, 0.5);
    let sender = transport_layer::Sender::new(tx_send, rx_send);
    let receiver = transport_layer::Receiver::new(rx_recv, tx_recv);
    let out = Arc::new(std::io::stdout());
    // let out2 = out.clone();
    let bytes_sent = Arc::new(AtomicUsize::new(0));
    let bs = bytes_sent.clone();
    let packets_sent = Arc::new(AtomicUsize::new(0));
    let ps = packets_sent.clone();
    let correct_buf = [0x1u8, 0x2, 0x3];
    thread::spawn(move || {
        let buf = [0x1u8, 0x2, 0x3];
        for _ in 0..10 {
            let len = match sender.send(&buf) {
                Ok(len) => len,
                Err(e) => {
                    writeln!(out.lock(), "Send error: {:?}", e).unwrap();
                    break
                },
            };
            bytes_sent.fetch_add(len, Ordering::SeqCst);
            packets_sent.fetch_add(1, Ordering::SeqCst);
            // writeln!(out.lock(), "Sent {} bytes", len).unwrap();
        }
        sender.simulation_end();
    });
    let bytes_recv = Arc::new(AtomicUsize::new(0));
    let br = bytes_recv.clone();
    let packets_recv_okay = Arc::new(AtomicUsize::new(0));
    let pro = packets_recv_okay.clone();
    thread::spawn(move || {
        let mut buf = [0u8; 256];
        loop {
            let len = match receiver.recv(&mut buf) {
                Ok(len) => len,
                Err(_e) => {
                    // writeln!(out2.lock(), "Receive error: {:?}", e).unwrap();
                    break
                },
            };
            if buf[..len] == correct_buf {
                packets_recv_okay.fetch_add(1, Ordering::SeqCst);
            } 
            bytes_recv.fetch_add(len, Ordering::SeqCst);
            // writeln!(out2.lock(), "Received {} bytes", len).unwrap();
        }
    });
    reliable.run();
    println!("Bytes sent: {}", bs.load(Ordering::SeqCst));
    println!("Bytes received: {}", br.load(Ordering::SeqCst));
    println!("Packets sent: {}", ps.load(Ordering::SeqCst));
    println!("Correct packets received: {}", pro.load(Ordering::SeqCst));
}
