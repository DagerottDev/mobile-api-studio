import json, os, pathlib, socket, ssl, subprocess, tempfile, time, shutil

WORK = pathlib.Path(tempfile.mkdtemp(prefix='mas-protocol-'))
ADDON = pathlib.Path(__file__).with_name('mas_bridge.py')

def port():
    with socket.socket() as s:
        s.bind(('127.0.0.1', 0))
        return s.getsockname()[1]

def wait_port(number):
    for _ in range(100):
        try:
            with socket.create_connection(('127.0.0.1', number), 0.1): return
        except OSError: time.sleep(0.1)
    raise RuntimeError(f'port {number} did not open')

upstream_port, proxy_port = port(), port()
cert, key = WORK/'cert.pem', WORK/'key.pem'
subprocess.run(['openssl','req','-x509','-newkey','rsa:2048','-nodes','-subj','/CN=localhost','-days','1','-keyout',str(key),'-out',str(cert)], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
server_js = WORK/'server.js'
server_js.write_text('''
const http2 = require('http2');
const crypto = require('crypto');
const fs = require('fs');
const s = http2.createSecureServer({key:fs.readFileSync(process.argv[2]), cert:fs.readFileSync(process.argv[3]), allowHTTP1:true});
s.on('stream', (stream, headers) => {
  if (headers[':path'] === '/grpc') {
    stream.respond({':status':200,'content-type':'application/grpc'}, {waitForTrailers:true});
    stream.on('wantTrailers',()=>stream.sendTrailers({'grpc-status':'0','x-test-trailer':'seen'}));
    stream.end(Buffer.from([0,0,0,0,2,104,105]));
  } else {stream.respond({':status':200}); stream.end('ok');}
});
s.on('upgrade',(req,socket)=>{
 const accept=crypto.createHash('sha1').update(req.headers['sec-websocket-key']+'258EAFA5-E914-47DA-95CA-C5AB0DC85B11').digest('base64');
 socket.write('HTTP/1.1 101 Switching Protocols\\r\\nUpgrade: websocket\\r\\nConnection: Upgrade\\r\\nSec-WebSocket-Accept: '+accept+'\\r\\n\\r\\n');
 socket.on('data',data=>{ if (data.length) socket.write(Buffer.from([0x81,0x00])); });
});
s.listen(Number(process.argv[4]), '127.0.0.1');
''')
server = subprocess.Popen(['node',str(server_js),str(key),str(cert),str(upstream_port)], stdout=(WORK/'server.out').open('wb'), stderr=subprocess.STDOUT)
proxy = None
try:
    wait_port(upstream_port)
    proxy_out = (WORK/'proxy.out').open('wb')
    env = os.environ.copy(); env['MAS_SESSION_ID']='protocol-check'; env.pop('MAS_RULE_SOCKET',None)
    proxy = subprocess.Popen(['mitmdump','--mode',f'reverse:https://127.0.0.1:{upstream_port}','--listen-host','127.0.0.1','--listen-port',str(proxy_port),'--set',f'confdir={WORK}/conf','--set','ssl_insecure=true','--set','connection_strategy=lazy','-s',str(ADDON)],env=env,stdout=proxy_out,stderr=subprocess.STDOUT)
    wait_port(proxy_port)
    client_js = WORK/'client.js'
    client_js.write_text('''
const http2=require('http2');
const client=http2.connect('https://127.0.0.1:'+process.argv[2],{rejectUnauthorized:false,servername:'localhost'});
client.on('error',e=>{console.error(e);process.exitCode=1});
client.on('connect',()=>{
 const req=client.request({':method':'POST',':path':'/grpc','content-type':'application/grpc'});
 let data=[]; req.on('response',h=>console.log('H2_RESPONSE',JSON.stringify(h)));
 req.on('trailers',h=>console.log('H2_TRAILERS',JSON.stringify(h)));
 req.on('data',b=>data.push(b));req.on('end',()=>{console.log('H2_BODY',Buffer.concat(data).toString('hex'));client.close()});
 req.end(Buffer.from([0,0,0,0,2,104,105]));
});
''')
    h2 = subprocess.run(['node',str(client_js),str(proxy_port)],capture_output=True,text=True,timeout=15)
    print('H2_CLIENT',h2.returncode,h2.stdout.strip(),h2.stderr.strip())
    context=ssl._create_unverified_context()
    with context.wrap_socket(socket.create_connection(('127.0.0.1',proxy_port),3),server_hostname='localhost') as sock:
        sock.settimeout(3)
        key64='dGhlIHNhbXBsZSBub25jZQ=='
        sock.sendall(f'GET /ws HTTP/1.1\r\nHost: localhost:{proxy_port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key64}\r\nSec-WebSocket-Version: 13\r\n\r\n'.encode())
        handshake=sock.recv(4096)
        print('WS_HANDSHAKE',handshake.split(b'\r\n',1)[0].decode(errors='replace'))
        for opcode,data in [(1,b'text'),(2,b'\x00\xff'),(1,b'')]:
            mask=b'ABCD'; masked=bytes(b^mask[i%4] for i,b in enumerate(data))
            sock.sendall(bytes([0x80|opcode,0x80|len(data)])+mask+masked)
            try: sock.recv(1024)
            except socket.timeout: pass
    time.sleep(1)
    proxy_out.flush()
    events=[]
    for line in (WORK/'proxy.out').read_text(errors='replace').splitlines():
        if line.startswith('MAS_EVENT '):
            try: events.append(json.loads(line[10:]))
            except ValueError: pass
    print('EVENTS',[(e['type'],e.get('opcode'),e.get('sequence')) for e in events])
    for e in events:
        if e['type']=='flow_completed':
            p=e['protocol']; print('PROTOCOL',p['requestHttpVersion'],p['responseHttpVersion'],p['requestTrailers'],p['responseTrailers'],p['clientConnection']['tlsVersion'],p['serverConnection']['tlsVersion'] if p['serverConnection'] else None)
        elif e['type']=='websocket_message': print('WS_EVENT',e['opcode'],e['body']['data_base64'],e['body']['is_binary'],e['body']['content_type'])
        elif e['type']=='websocket_closed': print('WS_CLOSED',e)
    assert h2.returncode==0 and 'H2_BODY 00000000026869' in h2.stdout
    assert any(e['type']=='flow_completed' and e['protocol']['requestHttpVersion']=='HTTP/2.0' and e['protocol']['responseTrailers'][0]['name']=='grpc-status' and e['protocol']['serverConnection']['peerCertificates'][0]['sha256'] for e in events)
    ws=[e for e in events if e['type']=='websocket_message' and e['from_client']]
    assert [(e['opcode'],e['body']['data_base64']) for e in ws]==[(1,'dGV4dA=='),(2,'AP8='),(1,'')]
    assert [e['body']['is_binary'] for e in ws]==[False,True,False]
    assert any(e['type']=='websocket_closed' for e in events)
finally:
    if proxy: proxy.terminate(); proxy.wait(timeout=5)
    server.terminate(); server.wait(timeout=5)
    shutil.rmtree(WORK)
