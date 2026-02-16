# Rust WebSocket Server -- Complete Notes

## 1. WebSocket Frame Types

According to RFC 6455, the main WebSocket frame types are:

-   Text
-   Binary
-   Ping
-   Pong
-   Close

In Axum, these are represented as:

    Message::Text(String)
    Message::Binary(Bytes)
    Message::Ping(Bytes)
    Message::Pong(Bytes)
    Message::Close(Option<CloseFrame>)

### Continuation Frames

Continuation frames exist at protocol level but are handled internally
by the library. You do not handle them manually.

------------------------------------------------------------------------

## 2. What is `split()`?

When you call:

    let (sender, receiver) = socket.split();

You divide the WebSocket into:

-   Sender (Sink)
-   Receiver (Stream)

### Why?

WebSocket is full duplex, meaning it can send and receive
simultaneously.

Without split: You would be forced into sequential communication.

With split: You can run independent read and write tasks concurrently.

This avoids Rust borrow checker issues where you cannot have multiple
mutable references at the same time.

------------------------------------------------------------------------

## 3. What is a Mutex?

Mutex = Mutual Exclusion.

It ensures only one task can access shared data at a time.

Example:

    Arc<Mutex<SplitSink<WebSocket, Message>>>

Why needed?

-   SplitSink requires mutable access
-   Multiple async tasks may try to send messages
-   Mutex ensures safe, exclusive access

------------------------------------------------------------------------

## 4. Types of Mutex in Rust

### std::sync::Mutex

-   Blocking
-   Not suitable for async

### tokio::sync::Mutex

-   Async-aware
-   Uses `.lock().await`
-   Does not block the thread

Used in async WebSocket servers.

------------------------------------------------------------------------

## 5. Why Arc?

Arc = Atomic Reference Counted pointer.

Allows multiple ownership of shared data safely across tasks.

------------------------------------------------------------------------

## 6. High Performance Design Note

Using Arc\<Mutex`<SplitSink>`{=html}\> works but is not optimal.

Production WebSocket servers: - Spawn dedicated write task - Use mpsc
channel - Avoid Mutex for better scalability

------------------------------------------------------------------------

## Final Architecture Overview

Tokio (Async Runtime) ↓ Hyper (HTTP Engine) ↓ Axum (Web Framework) ↓
WebSocket (Stream + Sink) ↓ Split into Sender + Receiver ↓ Optional
Arc + Mutex for shared access

------------------------------------------------------------------------

These notes summarize: - WebSocket frame types - Purpose of split() -
Why Mutex is needed - Difference between std::Mutex and tokio::Mutex -
Basic production architecture insight