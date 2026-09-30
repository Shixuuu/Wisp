// How far down the page you have read, for the grey that fills the tab.
(function () {
  if (window.top !== window) return;
  var last = -1, timer = null;
  function send() {
    timer = null;
    var room = document.documentElement.scrollHeight - innerHeight;
    var read = room > 0 ? Math.min(1, Math.max(0, scrollY / room)) : 0;
    read = Math.round(read * 100) / 100;
    if (read === last) return;
    last = read;
    try { webkit.messageHandlers.wispMeter.postMessage(read); } catch (e) {}
  }
  addEventListener('scroll', function () { if (!timer) timer = setTimeout(send, 80); }, { passive: true });
})();
