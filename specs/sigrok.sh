#!/bin/bash -eu

source fappa-v1.bash

sudo apt install -y \
  fappa-standard

# main build deps
sudo apt install -y \
  cmake \
  libusb-1.0-0-dev \
  libconfuse-dev

# python bindings
sudo apt install -y \
    swig \
    python-dev \
    libboost-dev \
    libboost-test-dev

# docs (sigh)
sudo apt install -y \
  doxygen

git-export 'git://developer.intra2net.com/libftdi' master d5c1622a2ff0c722c0dc59533748489b45774e55 .

fappa-cmake

sudo make install

export DEPENDS=a

fappa-package
