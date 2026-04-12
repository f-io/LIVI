#!/bin/bash
if [ -f /etc/apparmor.d/livi ]; then
  apparmor_parser -R /etc/apparmor.d/livi 2>/dev/null || true
  rm -f /etc/apparmor.d/livi
fi
