(function() {
  var b = {favicon_url: '/favicon.svg', app_name: 'Nosdesk'};
  try { Object.assign(b, JSON.parse(localStorage.getItem('nosdesk_branding_cache') || '{}')); } catch (e) {}
  var link = document.createElement('link');
  link.rel = 'icon';
  link.href = b.favicon_url;
  document.head.appendChild(link);
  document.title = b.app_name;
})();
