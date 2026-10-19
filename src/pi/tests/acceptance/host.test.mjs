import assert from "node:assert/strict";
import {spawn,spawnSync} from "node:child_process";
import {mkdtempSync,mkdirSync,readFileSync,writeFileSync,existsSync,cpSync,rmSync} from "node:fs";
import {join} from "node:path";
import {tmpdir} from "node:os";
import {fileURLToPath} from "node:url";
import {once} from "node:events";
import {test} from "node:test";
import {install} from "../../install/index.mjs";
const root=fileURLToPath(new URL("../../../../",import.meta.url));
import {runtime as prepared, verifyRuntime} from "../../install/artifact.mjs";

test("installed Pi loads the package, runs a native child, reloads, and shuts down", async t => {
verifyRuntime();
mkdirSync(join(root,"dist"),{recursive:true});
const work=mkdtempSync(join(tmpdir(),"dots-pi-host-"));
t.after(() => rmSync(work,{recursive:true,force:true}));
const runtime=join(work,"runtime");
cpSync(prepared,runtime,{recursive:true});
const manifest=JSON.parse(readFileSync(join(runtime,"package.json"),"utf8"));
const binary=process.env.PI_BIN??"pi";
const version=spawnSync(binary,["--version"],{encoding:"utf8",timeout:10000});
assert.equal(version.status,0,version.stderr);
assert.equal(version.stdout.trim(),manifest.devDependencies["@earendil-works/pi-coding-agent"],"installed host must match the development SDK");
const marker=join(work,"markers.jsonl");
const results=[];
class Client {
    constructor(extension,name){
        const agent=join(work,name);mkdirSync(agent,{recursive:true});
        this.pending=new Map();this.events=[];this.stderr="";this.next=0;this.buffer="";
        const env={...process.env,PI_CODING_AGENT_DIR:agent,PI_OFFLINE:"1",PI_TRIAL_MARKER:marker};
        for(const k of Object.keys(env))if(/API_KEY|TOKEN|SECRET|PASSWORD/.test(k))delete env[k];
        this.child=spawn(binary,["--mode","rpc","--no-session",...(extension ? ["--no-extensions"] : []),"--no-mcp","--no-context-files","--no-skills","--no-prompt-templates","--no-themes","--no-approve","--offline",...(extension ? ["-e",extension] : [])],{cwd:work,env,stdio:["pipe","pipe","pipe"]});
        this.exited=once(this.child,"exit");
        this.child.stderr.on("data",b=>{this.stderr=(this.stderr+b).slice(-65536);});
        this.child.stdout.on("data",b=>{
            this.buffer+=b;if(this.buffer.length>4*1024*1024){this.child.kill("SIGTERM");return;}
            for(let index;(index=this.buffer.indexOf("\n"))>=0;){
                const line=this.buffer.slice(0,index);this.buffer=this.buffer.slice(index+1);
                if(!line.trim())continue;
                let event;try{event=JSON.parse(line);}catch{continue;}
                this.events.push(event);
                if(event.type==="response" && this.pending.has(event.id)){
                    const pending=this.pending.get(event.id);this.pending.delete(event.id);clearTimeout(pending.timer);
                    event.success?pending.resolve(event):pending.reject(new Error(JSON.stringify(event)));
                }
            }
        });
        this.child.on("exit",()=>{for(const p of this.pending.values()){clearTimeout(p.timer);p.reject(new Error("Pi exited: "+this.stderr));}this.pending.clear();});
        this.child.stdin.on("error",()=>{});
    }
    call(type,data={}){
        const id=String(++this.next);
        return new Promise((resolve,reject)=>{
            const timer=setTimeout(()=>{this.pending.delete(id);reject(new Error("Pi command timeout: "+type+"\n"+this.stderr));},25000);
            this.pending.set(id,{resolve,reject,timer});this.child.stdin.write(JSON.stringify({id,type,...data})+"\n");
        });
    }
    async close(){
        this.child.stdin.end();
        const timer=setTimeout(()=>this.child.kill("SIGKILL"),10000);
        try{
            const [code,signal]=await this.exited;
            assert.equal(code,0,"Pi did not shut down cleanly: "+signal+" "+this.stderr);
            this.verify();
        }finally{clearTimeout(timer);}
        writeFileSync(join(work,"host-"+this.child.pid+".log"),this.stderr);
    }
    verify(){
        assert.ok(!this.events.some(e=>e.type==="agent_start"),"smoke must not start provider requests");
        const errors=this.events.filter(e=>e.type==="extension_error" || (e.type==="extension_ui_request" && e.method==="notify" && e.notifyType==="error"));
        assert.deepEqual(errors,[],"host extension errors");
        assert.doesNotMatch(this.stderr,/Failed to load extension|ERR_MODULE_NOT_FOUND|Cannot find module|stale after session/i);
    }
}
const expected=["context","memory","ui","workers","wakes","jobs"];
await install(join(work,"installed"));
const installed=new Client(undefined,"installed");
try {
    const response=await installed.call("get_commands");
    for(const name of expected)assert.ok(response.data.commands.some(c=>c.name===name),"installed resource missing: "+name);
    installed.verify();results.push({gate:"installed-settings-and-release-pointer-loading",passed:true});
} finally {await installed.close();}
const normal=new Client(runtime,"manifest");
try{
    const response=await normal.call("get_commands");
    for(const name of expected)assert.ok(response.data.commands.some(c=>c.name===name),"manifest resource missing: "+name);
    await normal.call("prompt",{message:"/jobs"});
    normal.verify();results.push({gate:"real-package-manifest-loading",passed:true});
}finally{await normal.close();}
// Hand-authored driver loads the identical artifact factories with a narrow
// registration observer. Disable automatic model turns, not job execution.
const driver=join(runtime,".trial-driver.ts");
writeFileSync(driver,`${manifest.pi.extensions.map((p,i)=>`import extension${i} from ${JSON.stringify("./"+p)};`).join("\n")}
import {appendFileSync,readFileSync} from "node:fs";
import {join} from "node:path";
import {createToolRenderers} from "@joshrwolf/pi-dots/ui";
export default function(pi){
 const marker=process.env.PI_TRIAL_MARKER;
 const mark=(data)=>appendFileSync(marker,JSON.stringify(data)+"\\n");
 const tools=new Map();
 const proxy=new Proxy(pi,{get(target,key){
  if(key==="registerTool")return tool=>{tools.set(tool.name,tool);target.registerTool(tool);};
  if(key==="sendMessage")return (message,options)=>target.sendMessage(message,{...options,triggerTurn:false});
  return Reflect.get(target,key);
 }});
 ${manifest.pi.extensions.map((_,i)=>`extension${i}(proxy);`).join("\n")}
 pi.on("session_start",()=>mark({event:"start"}));
 pi.on("session_shutdown",()=>mark({event:"shutdown"}));
 pi.registerCommand("lab-job",{description:"isolated packaged child-process acceptance",async handler(args,ctx){
  if(typeof createToolRenderers!=="function")throw new Error("public UI export missing");
  const started=await tools.get("job_start").execute("start",{command:"printf PACKAGE_CHILD_OK",cwd:ctx.cwd,timeoutMs:5000},undefined,undefined,ctx);
  const id=started.details.id;let job;
  const deadline=Date.now()+12000;
  do{
   const status=await tools.get("job_status").execute("status",{id},undefined,undefined,ctx);
   job=status.details.jobs[0];
   if(job.state==="finished")break;
   await new Promise(resolve=>setTimeout(resolve,50));
  }while(Date.now()<deadline);
  if(job.state!=="finished" || job.facts.root?.code!==0)throw new Error("packaged child did not finish: "+JSON.stringify(job));
  const output=readFileSync(join(job.directory,"output.log"),"utf8");
  if(output!=="PACKAGE_CHILD_OK")throw new Error("child capture mismatch");
  mark({event:"child-pass",id,publicExport:true});
 }});
 pi.registerCommand("lab-reload",{description:"isolated runtime reload",async handler(args,ctx){await ctx.reload();}});
}
`);
const observed=new Client(driver,"lifecycle");
try{
    await observed.call("get_commands");
    await observed.call("prompt",{message:"/lab-job"});
    await observed.call("prompt",{message:"/lab-reload"});
    await observed.call("prompt",{message:"/lab-job"});
    observed.verify();
}finally{await observed.close();}
const marks=readFileSync(marker,"utf8").trim().split("\n").map(JSON.parse);
assert.equal(marks.filter(m=>m.event==="child-pass").length,2);
assert.ok(marks.filter(m=>m.event==="start").length>=2);
assert.ok(marks.filter(m=>m.event==="shutdown").length>=2);
results.push({gate:"packaged-child-public-export-reload-shutdown",passed:true});
assert.ok(!existsSync(join(work,"manifest/settings.json")),"manifest test must not register packages");
console.log(JSON.stringify({passed:true,results},null,2));
});
