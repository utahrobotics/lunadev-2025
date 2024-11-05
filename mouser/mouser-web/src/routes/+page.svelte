<script>
    let socket = null;
    socket = new WebSocket("/ws/");
    // Connection opened
    socket.addEventListener("open", (event) => {
        socket.send("W");
    });

    let queued = true;
    let robotName = "";

    // Listen for messages
    socket.addEventListener("message", (event) => {
        if (event.data === "Queued") {
            queued = true;
        } else {
            robotName = event.data;
            queued = false;
        }
    });
</script>
<svelte:head>
    <title>Mouse Robot</title>
</svelte:head>
<h1>Mouse Robot</h1>
<div id="mouse-cam">
    <embed src="http://localhost:1984/stream.html?src=macos_facetime&mode=webrtc,mse,hls,mjpeg">
</div>

{#if queued}
    <p>Waiting in queue...</p>
{:else}
    <p>Connected to {robotName}</p>
    <section id="controls">
        <button on:click={() => socket.send("W")}>W</button>
        <div>
            <button on:click={() => socket.send("A")}>A</button>
            <button on:click={() => socket.send("S")}>S</button>
            <button on:click={() => socket.send("D")}>D</button>
        </div>
    </section>
{/if}


<style>
    #mouse-cam {
        width: calc(100% - 3rem);
    }

    #mouse-cam embed {
        width: 100%;
        height: auto;
    }

    :global(body) {
        display: flex;
        flex-direction: column;
        align-items: center;
    }

    #controls {
        display: flex;
        flex-direction: column;
        align-items: center;
    }

    #controls button {
        font-size: 2rem;
        padding-top: 1rem;
        padding-bottom: 1rem;
        padding-left: 1.6rem;
        padding-right: 1.6rem;
        margin: 0.5rem;
    }

    #controls div {
        display: flex;
        flex-direction: row;
    }

    h1 {
        font-weight: bold;
        font-size: 1.5rem;
    }

    button {
        background-color: gray;
        border-radius: 1rem;
    }
</style>