// Shared by every page in docs/: copy buttons everywhere, the calculator where it exists.
(function(){
  function $(id){return document.getElementById(id)}
  var names=["Chip asleep","Light ramp","Lamp after prayer","Alarm audio"];
  function fmt(n){return n.toLocaleString('en-US',{maximumFractionDigits:1})}
  function calc(){
    var cap=+$('cap').value*+$('cells').value*0.8;
    var st=+$('stby').value*24;
    var lamp=+$('lamp').value, cpu=+$('cpu').value;
    var ramp=(lamp/3+cpu)*(+$('ramp').value)/60;
    var hold=(lamp+cpu)*(+$('hold').value)/60;
    var buz=(+$('audma').value+cpu)*(+$('aud').value)/60;
    var parts=[st,ramp,hold,buz], tot=st+ramp+hold+buz;
    var days=tot>0?cap/tot:0;
    var txt=Math.round(days)+' days';
    $('days').textContent=txt;
    if($('heroDays'))$('heroDays').textContent='~'+txt;
    $('daysNote').textContent='between charges, using '+fmt(tot)+' mAh per day';
    var max=Math.max.apply(null,parts)||1, h='';
    for(var i=0;i<4;i++){
      h+='<div class="bar"><span>'+names[i]+'</span><i style="width:'+Math.max(1,parts[i]/max*100)+'%"></i><span>'+fmt(parts[i])+' mAh</span></div>';
    }
    $('bars').innerHTML=h;
  }
  if($('cap')){
    ['cap','cells','stby','lamp','ramp','hold','cpu','aud','audma'].forEach(function(id){$(id).addEventListener('input',calc)});
    calc();
  }
  document.querySelectorAll('button[data-copy]').forEach(function(b){
    b.addEventListener('click',function(){
      var el=$(b.getAttribute('data-copy')), t=el.innerText;
      function sel(){var r=document.createRange();r.selectNodeContents(el);var s=getSelection();s.removeAllRanges();s.addRange(r);b.textContent='Selected: copy it'}
      try{navigator.clipboard.writeText(t).then(function(){b.textContent='Copied'},sel)}catch(e){sel()}
    });
  });
})();
