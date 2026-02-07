import subprocess
import logging
from dippy.core.config import Config


def run_notifier(config: Config, idle: bool = False) -> str | None:
    """
    Execute the notifier-command and return its output wrapped in a tag.
    If idle=True, append --idle to the command.
    """
    if not config.notifier_command:
        return None

    cmd = config.notifier_command
    if idle:
        cmd += " --idle"

    try:
        # Run command, capture stdout
        # Using shell=True as the command may contain arguments
        result = subprocess.run(
            cmd,
            shell=True,
            capture_output=True,
            text=True,
            timeout=300 if idle else 30,  # Longer timeout for idle/wait mode
        )

        output = result.stdout.strip()
        if output:
            return f"<notification_note>\n{output}\n</notification_note>"

    except subprocess.TimeoutExpired:
        if not idle:
            logging.warning(f"Notifier command timed out: {cmd}")
    except Exception as e:
        logging.warning(f"Notifier command failed: {e}")

    return None
