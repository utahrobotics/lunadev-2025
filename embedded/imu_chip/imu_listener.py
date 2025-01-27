import socket

def start_server():
    host = '0.0.0.0'  # Listen on all available interfaces
    port = 30600

    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as server_socket:
        server_socket.bind((host, port))
        server_socket.listen(1)
        print(f"Listening on {host}:{port}")

        while True:
            client_socket, client_address = server_socket.accept()
            with client_socket:
                print(f"Connection from {client_address}")
                while True:
                    try:
                        data = client_socket.recv(1024)
                    except ConnectionResetError:
                        print(f"Connection reset by {client_address}")
                        break
                    if not data:
                        break
                    print(f"Received: {data.decode('utf-8')}")

if __name__ == "__main__":
    start_server()