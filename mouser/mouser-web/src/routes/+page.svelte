<script lang=ts>
	import { enhance } from "$app/forms";

    let socket: WebSocket | null = null;

    let queued = true;
    let robotName = "";

    function connect() {
        socket = new WebSocket("/ws/");
        // Listen for messages
        socket.addEventListener("message", (event) => {
            if (event.data === "Queued") {
                queued = true;
            } else {
                robotName = event.data;
                queued = false;
            }
        });

        socket.addEventListener("close", (event) => {
            socket = null;
        });
    }

    let ip_logged = false;
</script>
<svelte:head>
    <title>Mouse Robot</title>
</svelte:head>
<h1>Mouse Robot</h1>
<div id="mouse-cam">
    <embed src="http://localhost:1984/stream.html?src=macos_facetime&mode=webrtc,mse,hls,mjpeg">
</div>

{#if socket === null}
    <button on:click={connect}>Join Queue</button>
{:else if queued}
    <p>Waiting in queue...</p>
{:else}
    <p>Connected to {robotName}</p>
    <section id="controls">
        <button on:click={() => socket!.send("W")}>W</button>
        <div>
            <button on:click={() => socket!.send("A")}>A</button>
            <button on:click={() => socket!.send("S")}>S</button>
            <button on:click={() => socket!.send("D")}>D</button>
        </div>
    </section>
{/if}

{#if ip_logged}
    <h1>Participation</h1>
    <p>Thank you for participating!</p>
{:else}
    <form action="/log_ip" method="post" use:enhance={() => {
        ip_logged = true;
    }}>
        <h1>Participation</h1>
        <p>
            Mouse Robots is an outreach event by Utah Student Robotics, and we would love it if you could participate!
            You are always welcome to use this web app, but clicking the button below will allow us to log your IP address
            to help us keep track of how many people have visited this site. Your IP address is kept private, and only the
            number of unique IPs is examined, not their geographical location or any other identifying information.
        </p>
        <button type="submit">Participate</button>
    </form>
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