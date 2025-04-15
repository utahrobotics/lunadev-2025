FROM rust

# Update the package list and install basic packages
RUN apt-get update && apt-get install -y zip cmake ninja-build kmod libclang-dev clang mold

RUN apt-get -y install libv4l-dev libudev-dev

ENV VCPKG_FORCE_SYSTEM_BINARIES=1
RUN git clone https://github.com/Microsoft/vcpkg.git
RUN cd vcpkg && ./bootstrap-vcpkg.sh
RUN ./vcpkg/vcpkg integrate install
RUN ./vcpkg/vcpkg install realsense2

RUN git clone https://github.com/AprilRobotics/apriltag.git
RUN cd apriltag && \
    cmake -B build -DCMAKE_BUILD_TYPE=Release && \
    cmake --build build --target install

# Install vulkan
RUN apt-get install -y \
    libxext6 \
    libvulkan1 \
    libvulkan-dev \
    vulkan-tools

RUN apt-get -y install ffmpeg
RUN cargo install --force cargo-make

ENV PKG_CONFIG_PATH=/vcpkg/installed/arm64-linux/lib/pkgconfig/

# RUN apt-get install -y obs-studio
# RUN apt-get install -y pipx
# RUN pipx install obs-cli
# RUN pipx ensurepath

# Install go
# RUN curl -L https://go.dev/dl/go1.23.3.linux-amd64.tar.gz > go1.23.3.linux-amd64.tar.gz && \
#     tar -C /usr/local -xzf go1.23.3.linux-amd64.tar.gz && \
#     rm go1.23.3.linux-amd64.tar.gz

# ENV PATH=$PATH:/usr/local/go/bin

# RUN go install github.com/muesli/obs-cli@latest
# RUN apt-get install linux-headers-generic