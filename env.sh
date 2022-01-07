#!/bin/sh
export SSID="funkgeloet"
export PSK=$(security find-generic-password -w -a "$SSID")
