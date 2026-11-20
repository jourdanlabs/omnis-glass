#[test]
fn canvas_coordinates_follow_device_frame_size() {
    let script = r#"
const fs = require('fs'), vm = require('vm'), assert = require('assert');
const listeners = {};
const canvas = {getContext:()=>({drawImage(){}}), addEventListener:(n,fn)=>listeners[n]=fn, getBoundingClientRect:()=>({left:10,top:20,width:744,height:612}), focus(){}};
const elements = {viewport:canvas, 'url-bar':{addEventListener(){}}, 'mode-badge':{textContent:'MIRROR'}, 'rail-list':{}, 'btn-take':{}, 'btn-release':{}, status:{}, capture:{}};
for (const n of ['back','forward','reload']) elements['btn-'+n]={};
const document={getElementById:n=>elements[n], querySelectorAll:()=>[], addEventListener(){}, createElement:()=>({})};
let sent=[];
const window={__GLASS_PANE_AUTH_TOKEN:'test',ipc:{postMessage:s=>sent.push(JSON.parse(s.split('|')[1]))}};
class Image { set src(_) {} }
vm.runInNewContext(fs.readFileSync(process.argv[1], 'utf8'), {document, window, Image});
window.__glassPane.setMode('HUMAN');
window.__glassPane.drawFrame({data:'',metadata:{deviceWidth:1280,deviceHeight:720}});
listeners.mousedown({clientX:196,clientY:326,button:0,buttons:1,detail:1,preventDefault(){}});
const mouse=sent.find(x=>x.type==='mouse');
assert.strictEqual(mouse.params.x,320); assert.strictEqual(mouse.params.y,360);
"#;
    let path = format!("{}/../glass-pane-ui/ui/pane.js", env!("CARGO_MANIFEST_DIR"));
    let output = std::process::Command::new("node").args(["-e", script, &path]).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn mouse_moves_are_coalesced_without_dropping_buttons() {
    let script = r#"
const fs=require('fs'),vm=require('vm'),assert=require('assert');
let now=0, timers=[], sent=[], nextTimer=0; const listeners={};
const canvas={getContext:()=>({drawImage(){}}),addEventListener:(n,f)=>listeners[n]=f,getBoundingClientRect:()=>({left:0,top:0,width:100,height:100}),focus(){}};
const elements={viewport:canvas,'url-bar':{addEventListener(){}},'mode-badge':{textContent:'MIRROR'},'rail-list':{},'btn-take':{},'btn-release':{},status:{},capture:{}};
for(const n of ['back','forward','reload']) elements['btn-'+n]={};
const document={getElementById:n=>elements[n],querySelectorAll:()=>[],addEventListener(){},createElement:()=>({})};
const window={__GLASS_PANE_AUTH_TOKEN:'test',ipc:{postMessage:s=>sent.push({time:now,cmd:JSON.parse(s.split('|')[1])})}};
class Image { set src(_) {} }
const context={document,window,Image,Date:{now:()=>now},setTimeout:(f,ms)=>{const id=++nextTimer;timers.push({id,at:now+ms,f});return id},clearTimeout:id=>{timers=timers.filter(x=>x.id!==id)}};
function advance(t){now=t;for(const x of timers.filter(x=>x.at<=now)) x.f();timers=timers.filter(x=>x.at>now);}
vm.runInNewContext(fs.readFileSync(process.argv[1],'utf8'),context);window.__glassPane.setMode('HUMAN');window.__glassPane.drawFrame({data:'',metadata:{deviceWidth:100,deviceHeight:100}});
function event(x){return {clientX:x,clientY:0,button:0,buttons:1,detail:1,preventDefault(){}}}
listeners.mousemove(event(1));advance(0);now=5;listeners.mousemove(event(2));now=10;listeners.mousemove(event(3));
listeners.mousedown(event(3));now=12;listeners.mousemove(event(4));now=15;listeners.mouseup(event(4));advance(60);
const mouse=sent.filter(x=>x.cmd.type==='mouse');
assert.deepStrictEqual(mouse.map(x=>x.cmd.params.type),['mouseMoved','mouseMoved','mousePressed','mouseMoved','mouseReleased']);
assert.deepStrictEqual(mouse.filter(x=>x.cmd.params.type==='mouseMoved').map(x=>x.cmd.params.x),[1,3,4]);
assert.deepStrictEqual(mouse.map(x=>x.time),[0,10,10,15,15]);
"#;
    let path = format!("{}/../glass-pane-ui/ui/pane.js", env!("CARGO_MANIFEST_DIR"));
    let output = std::process::Command::new("node").args(["-e", script, &path]).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}
