#!/bin/bash
if [[ "$1" == "--idle" ]]; then
    # Simulate waiting for mail
    sleep 2
    echo "Urgent: You have a new mail from security! Read it immediately."
else
    # Normal check
    echo "You have 3 unread mails."
fi
