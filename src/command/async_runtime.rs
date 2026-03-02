//! Generic async runtime pattern for long-running services
//!
//! This module demonstrates the pattern used by async services in this codebase.
//! It provides a template for building long-running services that need:
//! - Separate worker thread with Tokio async runtime
//! - Command/event communication via mpsc channels
//! - Graceful lifecycle management
//!
//! # Example Services Using This Pattern
//!
//! - **DiffServer** (`src/command/diff_server.rs`) - HTTP server for viewing git diffs
//! - Future: LSP server, file watchers, build servers, etc.
//!
//! # Pattern Overview
//!
//! Each service following this pattern consists of:
//!
//! 1. **Command Enum** - Messages sent TO the service
//!    ```rust,ignore
//!    #[derive(Debug, Clone)]
//!    pub enum MyServiceCommand {
//!        Start,
//!        Stop,
//!        DoSomething(String),
//!    }
//!    ```
//!
//! 2. **Event Enum** - Messages sent FROM the service
//!    ```rust,ignore
//!    #[derive(Debug, Clone)]
//!    pub enum MyServiceEvent {
//!        Started,
//!        Stopped,
//!        Progress(String),
//!        Error(String),
//!    }
//!    ```
//!
//! 3. **Handle Struct** - Interface for the main thread
//!    ```rust,ignore
//!    pub struct MyServiceHandle {
//!        pub command_tx: mpsc::Sender<MyServiceCommand>,
//!        pub event_rx: mpsc::Receiver<MyServiceEvent>,
//!        _worker_thread: Option<thread::JoinHandle<()>>,
//!    }
//!    ```
//!
//! 4. **Worker Struct** - Runs on separate thread with Tokio runtime
//!    ```rust,ignore
//!    struct MyServiceWorker {
//!        command_rx: mpsc::Receiver<MyServiceCommand>,
//!        event_tx: mpsc::Sender<MyServiceEvent>,
//!        tokio_runtime: tokio::runtime::Runtime,
//!        // Service-specific state...
//!    }
//!    ```
//!
//! # Why This Pattern?
//!
//! **Simple and Explicit**
//! - Each service is self-contained with its own types
//! - No complex trait bounds or generic constraints
//! - Easy to understand and debug
//!
//! **Easy to Duplicate**
//! - Copy the pattern for a new service
//! - Customize command/event enums for your needs
//! - Add service-specific logic without affecting others
//!
//! **Clean Separation**
//! - Worker runs on separate thread with dedicated Tokio runtime
//! - Main thread never blocks on async operations
//! - Commands and events provide clean interface
//!
//! # Implementation Steps
//!
//! To create a new service using this pattern:
//!
//! 1. **Define your enums**
//!    ```rust,ignore
//!    pub enum MyCommand { Start, Stop }
//!    pub enum MyEvent { Started, Error(String) }
//!    ```
//!
//! 2. **Create the Handle**
//!    ```rust,ignore
//!    pub struct MyHandle {
//!        pub command_tx: mpsc::Sender<MyCommand>,
//!        pub event_rx: mpsc::Receiver<MyEvent>,
//!        _worker_thread: Option<thread::JoinHandle<()>>,
//!    }
//!
//!    impl MyHandle {
//!        pub fn new() -> Result<Self, String> {
//!            let (command_tx, command_rx) = mpsc::channel();
//!            let (event_tx, event_rx) = mpsc::channel();
//!
//!            let worker = MyWorker {
//!                command_rx,
//!                event_tx,
//!                tokio_runtime: tokio::runtime::Builder::new_multi_thread()
//!                    .enable_all()
//!                    .build()
//!                    .map_err(|e| format!("Failed to build runtime: {}", e))?,
//!            };
//!
//!            let worker_thread = thread::Builder::new()
//!                .name("my-service".to_string())
//!                .spawn(move || worker.run())
//!                .map_err(|e| format!("Failed to spawn thread: {}", e))?;
//!
//!            Ok(Self {
//!                command_tx,
//!                event_rx,
//!                _worker_thread: Some(worker_thread),
//!            })
//!        }
//!    }
//!    ```
//!
//! 3. **Implement the Worker**
//!    ```rust,ignore
//!    struct MyWorker {
//!        command_rx: mpsc::Receiver<MyCommand>,
//!        event_tx: mpsc::Sender<MyEvent>,
//!        tokio_runtime: tokio::runtime::Runtime,
//!        // Add service-specific state here
//!    }
//!
//!    impl MyWorker {
//!        fn run(mut self) {
//!            loop {
//!                match self.command_rx.recv() {
//!                    Ok(MyCommand::Start) => self.handle_start(),
//!                    Ok(MyCommand::Stop) => self.handle_stop(),
//!                    Err(_) => break, // Main thread exited
//!                }
//!            }
//!        }
//!
//!        fn handle_start(&mut self) {
//!            // Spawn async tasks using self.tokio_runtime
//!            self.tokio_runtime.spawn(async move {
//!                // Your async code here
//!            });
//!
//!            // Send events back
//!            let _ = self.event_tx.send(MyEvent::Started);
//!        }
//!
//!        fn handle_stop(&mut self) {
//!            // Cleanup logic
//!        }
//!    }
//!    ```
//!
//! 4. **Integrate with App**
//!    - Add handle to App struct
//!    - Send commands via handle.command_tx
//!    - Poll events via handle.event_rx.try_recv()
//!
//! # Real-World Example
//!
//! See `src/command/diff_server.rs` for a complete implementation that:
//! - Starts an HTTP server on a worker thread
//! - Uses Tokio runtime for async HTTP handling
//! - Supports graceful shutdown
//! - Reports status via events
//!
//! # Notes
//!
//! - Worker thread and Tokio runtime are automatically cleaned up when Handle is dropped
//! - Use `try_recv()` in the main loop to poll for events without blocking
//! - Keep command/event enums simple - they need to be `Clone` for channel sending
//! - Service-specific state lives in the Worker struct, not shared with main thread

use std::sync::mpsc;
use std::thread;

// This module is intentionally minimal - it serves as documentation
// for the async runtime pattern. Real implementations are in separate
// modules like diff_server.rs.

/// Example command enum showing the pattern
#[allow(dead_code)]
#[derive(Debug, Clone)]
enum ExampleCommand {
    Start,
    Stop,
    DoWork(String),
}

/// Example event enum showing the pattern
#[allow(dead_code)]
#[derive(Debug, Clone)]
enum ExampleEvent {
    Started,
    Stopped,
    WorkComplete(String),
    Error(String),
}

/// Example handle showing the pattern
#[allow(dead_code)]
struct ExampleHandle {
    command_tx: mpsc::Sender<ExampleCommand>,
    event_rx: mpsc::Receiver<ExampleEvent>,
    _worker_thread: Option<thread::JoinHandle<()>>,
}

/// Example worker showing the pattern
#[allow(dead_code)]
struct ExampleWorker {
    command_rx: mpsc::Receiver<ExampleCommand>,
    event_tx: mpsc::Sender<ExampleEvent>,
    tokio_runtime: tokio::runtime::Runtime,
}

#[allow(dead_code)]
impl ExampleWorker {
    fn run(mut self) {
        loop {
            match self.command_rx.recv() {
                Ok(ExampleCommand::Start) => self.handle_start(),
                Ok(ExampleCommand::Stop) => self.handle_stop(),
                Ok(ExampleCommand::DoWork(data)) => self.handle_work(data),
                Err(_) => break, // Main thread exited
            }
        }
    }

    fn handle_start(&mut self) {
        // Example: spawn async task
        let event_tx = self.event_tx.clone();
        self.tokio_runtime.spawn(async move {
            // Do async work...
            let _ = event_tx.send(ExampleEvent::Started);
        });
    }

    fn handle_stop(&mut self) {
        let _ = self.event_tx.send(ExampleEvent::Stopped);
    }

    fn handle_work(&mut self, data: String) {
        let event_tx = self.event_tx.clone();
        self.tokio_runtime.spawn(async move {
            // Process data asynchronously...
            let _ = event_tx.send(ExampleEvent::WorkComplete(data));
        });
    }
}
