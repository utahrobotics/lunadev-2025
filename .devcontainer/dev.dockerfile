FROM rust

# Update the package list and install basic packages
RUN apt-get update && apt-get install -y zip cmake ninja-build kmod libclang-dev

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

ENV PKG_CONFIG_PATH=/vcpkg/installed/arm64-linux/lib/pkgconfig/


