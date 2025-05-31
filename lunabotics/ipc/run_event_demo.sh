#!/bin/bash

echo "🚀 IPC Event Coordination Demo"
echo "=============================="
echo ""
echo "This demo shows the event-based coordination between sender and receiver."
echo "The sender will wait for the receiver's ready signal before starting."
echo ""
echo "Instructions:"
echo "1. First, run this in one terminal to start the receiver:"
echo "   cargo run --example event_receiver"
echo ""
echo "2. Then, in another terminal, run the sender:"
echo "   cargo run --example event_sender"
echo ""
echo "3. Watch how the sender waits for the receiver's ready signal!"
echo ""
echo "Building examples first..."
cargo build --examples

if [ $? -eq 0 ]; then
    echo ""
    echo "✅ Examples built successfully!"
    echo ""
    echo "Now you can run the commands above in separate terminals."
    echo "Or run this script with an argument to start one of them:"
    echo "  ./run_event_demo.sh receiver   # Start the receiver"
    echo "  ./run_event_demo.sh sender     # Start the sender"
else
    echo "❌ Build failed. Please check the errors above."
    exit 1
fi

if [ "$1" = "receiver" ]; then
    echo ""
    echo "🎯 Starting receiver..."
    cargo run --example event_receiver
elif [ "$1" = "sender" ]; then
    echo ""
    echo "📤 Starting sender..."
    cargo run --example event_sender
fi 