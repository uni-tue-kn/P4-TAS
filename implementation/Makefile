CFLAGS=-D__STREAM_ID__=3 -D__STREAM_ID_SIZE__=4096 -D__STREAM_GATE_SIZE__=2048


TARGET ?= tofino2
SWITCHD = tf1
TOFINO_VERSION = 1

MAKEFILE_DIR := $(dir $(abspath $(lastword $(MAKEFILE_LIST))))
P4_FILE := $(realpath $(MAKEFILE_DIR)/p4_tas.p4)

ifeq ($(TARGET), tofino2)
    SWITCHD = tf2
    TOFINO_VERSION = 2
    TOFINO_FLAG = -DTOFINO2=ON
else
    TOFINO_FLAG = -DTOFINO=ON
endif

compile:
	sudo -E rm -rf ${SDE}/build/p4-build/p4_tas
	sudo -E mkdir -p ${SDE}/build/p4-build/p4_tas
	cd ${SDE}/build/p4-build/p4_tas && sudo -E cmake ${SDE}/p4studio \
	 -D__TARGET_TOFINO__=${TOFINO_VERSION}                         \
	 -DCMAKE_MODULE_PATH=${SDE}/cmake                              \
	 -DCMAKE_INSTALL_PREFIX=${SDE_INSTALL}                         \
	 -DP4_PATH=$(P4_FILE)                                          \
	 -DP4_NAME=p4_tas                                         \
	 -DP4_LANG=p4_16                                               \
	 $(CFLAGS)													   \
	 $(TOFINO_FLAG)
	cd ${SDE}/build/p4-build/p4_tas && sudo -E make -j install

start:
	sudo -E ${SDE}/run_switchd.sh -p p4_tas --arch ${SWITCHD}

compile-t2:
	sudo -E ${SDE_INSTALL}/bin/bf-p4c -D__TARGET_TOFINO__=2 $(CFLAGS) --target tofino2-t2na --arch tna2 -o /opt/tas p4_tas.p4
	sudo -E cp /opt/tas/p4_tas.conf ${SDE_INSTALL}/share/p4/targets/tofino2/p4_tas.conf