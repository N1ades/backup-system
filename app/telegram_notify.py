import urllib.request
import urllib.parse
from app.config import Config


def send(cfg: Config, text: str):
    """Fire-and-forget notification. No `requests` dep needed for one POST -
    urllib is stdlib and this is the only HTTP call in the whole app."""
    if not cfg.telegram_token or not cfg.telegram_chat_id:
        return
    url = f"https://api.telegram.org/bot{cfg.telegram_token}/sendMessage"
    data = urllib.parse.urlencode({"chat_id": cfg.telegram_chat_id, "text": text}).encode()
    try:
        urllib.request.urlopen(urllib.request.Request(url, data=data), timeout=10)
    except OSError:
        pass  # notification failure must never break/abort a backup run
