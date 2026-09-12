// Prod request policy: a composition rendered here must not reach services on this box or its
// networks. Applied browser-wide (every target, workers included) rather than through CDP
// request interception, which pauses module-worker imports forever under puppeteer 25.
//
// Two layers, because Chrome never consults a PAC script for loopback and link-local hosts
// (its implicit proxy-bypass rules, which `<-loopback>` can't switch off under PAC): those go
// to the host resolver as unresolvable; everything else goes through the PAC, which routes
// denied hosts to a closed local port so they fail instead of falling back to a direct
// connection. `file:` needs no rule: a page served over https can't load it.
const POLICY_PAC = `function FindProxyForURL(url, host) {
  var deny = 'PROXY 127.0.0.1:1';
  if (host === 'localhost' || dnsDomainIs(host, '.localhost') || host.indexOf(':') !== -1 ||
      /^\\d+\\.\\d+\\.\\d+\\.\\d+$/.test(host)) return deny;
  var ips = (dnsResolveEx(host) || '').split(';');
  for (var i = 0; i < ips.length; i++) {
    var ip = ips[i];
    if (!ip) continue;
    if (ip === '::1' || /^f[cd]/i.test(ip) || /^fe[89ab]/i.test(ip) || /^::ffff:/i.test(ip)) return deny;
    if (isInNet(ip, '127.0.0.0', '255.0.0.0') || isInNet(ip, '10.0.0.0', '255.0.0.0') ||
        isInNet(ip, '172.16.0.0', '255.240.0.0') || isInNet(ip, '192.168.0.0', '255.255.0.0') ||
        isInNet(ip, '169.254.0.0', '255.255.0.0') || isInNet(ip, '100.64.0.0', '255.192.0.0') ||
        isInNet(ip, '0.0.0.0', '255.0.0.0')) return deny;
  }
  return 'DIRECT';
}`;
const UNRESOLVABLE = ['localhost', '*.localhost', '127.*', '[::1]', '0.0.0.0', '169.254.*'];
export const POLICY_ARGS = [
  `--host-resolver-rules=${UNRESOLVABLE.map(h => `MAP ${h} ~NOTFOUND`).join(',')}`,
  `--proxy-pac-url=data:application/x-ns-proxy-autoconfig;base64,${Buffer.from(POLICY_PAC).toString('base64')}`,
];
