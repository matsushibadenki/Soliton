"""Compare repeated qualification pilots; never create startup evidence."""
import hashlib
import json
import math
from pathlib import Path
import statistics
import sys


def geometric(values):
    return math.exp(statistics.mean(math.log(value) for value in values))


def summarize(report):
    assert report['passed'] and report['certification'] is False
    assert report['shutdownClean'] and report['counterStopped'] and report['userStatePreserved']
    assert report['identityBefore']==report['identityAfter']
    micro=report.get('workloadKind')=='micro'
    assert len(report['cases'])==(52 if micro else 65) and all(c['exact'] for c in report['cases'])
    assert len(report['samples'])==(13 if micro else 26)
    warm=[c['elapsedMs'] for c in report['cases'] if c['phase'] in {'warm-before','warm-after'}]
    long=[c['elapsedMs'] for c in report['cases'] if c['phase'] in {'anchor-cold','churn','revisit'}]
    assert len(warm)==36 and len(long)==(4 if micro else 17)
    return dict(warmMedianMs=statistics.median(warm),longMissMedianMs=statistics.median(long),
        physicalPeakBytes=max(s['physicalBytes'] for s in report['samples']),
        physicalPrimaryPeakBytes=max(s['physicalBytes'] for s in report['samples'] if not s.get('phase','').startswith('seeded-')),
        cachePeakBytes=max(s['kvCache']['allocator']['cache_bytes'] for s in report['samples']),
        idlePhysicalBytes=report['samples'][-1]['physicalBytes'])


def compare(paths):
    reports=[json.loads(Path(p).read_bytes()) for p in paths]
    micro=reports[0].get('workloadKind')=='micro'
    assert all((r.get('workloadKind')=='micro')==micro for r in reports),'do not mix different workload cohorts'
    expected=[0,256,512,768,768,512,256,0] if micro else [0,256,512,512,256,0,768,768,256,0]
    assert [r['allocatorCacheMiB'] for r in reports]==expected
    if micro:
        assert len({r['workloadSha256'] for r in reports})==1
        assert len({r['fixturesSha256'] for r in reports})==1
        assert len({r['hostGuardSha256'] for r in reports})==1
        assert all(r['hostGuardMode']=='liveBackgroundCPU' for r in reports)
    signatures=[[(c['phase'],c['index'],c['promptTokens'],c['cachedTokens']) for c in r['cases']] for r in reports]
    assert all(s==signatures[0] for s in signatures),'workload/count/reuse mismatch'
    identity=reports[0]['identityBefore']
    assert all(all(v==r['identityBefore'][k] for k,v in identity.items() if k!='environment_sha256') for r in reports)
    for r in reports:
        if micro:
            values=[c['cpuAveragePercent'] for c in r['cases'] if c['phase']!='prime']+[s['cpuBusyPercent'] for s in r['samples']]
            assert values and all(type(v) in {int,float} and math.isfinite(v) and 0<=v<=100 for v in values)
            assert max(values)<=50 and all(c['cpuSustainedHighSeconds']<1 for c in r['cases'] if c['phase']!='prime'),'live CPU episode invalidates latency comparison'
        else:
            loads=[r.get('loadAtStart',[0])[0],r.get('loadAtEnd',[0])[0]]+[s.get('load',[0])[0] for s in r['samples']]+[c.get(key,0) for c in r['cases'] for key in ['loadBefore','loadAfter']]
            assert max(loads)<=10,'host load episode invalidates latency comparison'
    runs=[dict(path=str(p),budget=r['allocatorCacheMiB'],**summarize(r)) for p,r in zip(paths,reports)]
    def aggregate(indices):
        subset=[runs[i] for i in indices]
        return dict(repeats=len(subset),warmMedianMs=geometric([r['warmMedianMs'] for r in subset]),longMissMedianMs=geometric([r['longMissMedianMs'] for r in subset]),physicalPeakBytes=max(r['physicalPeakBytes'] for r in subset),physicalPrimaryPeakBytes=max(r['physicalPrimaryPeakBytes'] for r in subset),cachePeakBytes=max(r['cachePeakBytes'] for r in subset))
    groups=({'0':aggregate([0,7]),'256':aggregate([1,6]),'512':aggregate([2,5]),'768':aggregate([3,4])} if micro else {'0':aggregate([0,5,9]),'256':aggregate([1,4,8]),'512':aggregate([2,3]),'768':aggregate([6,7])})
    blocks=[(512,[1,6],[0,7]),(768,[1,6],[0,7])] if micro else [(512,[1,4],[0,5]),(768,[4,8],[5,9])]
    eligible=[]
    for budget,controls,references in blocks:
        g=groups[str(budget)];base=aggregate(controls);ref=aggregate(references)
        g['pairedControl']=base;g['pairedDefault']=ref
        g['warmReductionPercent']=(1-g['warmMedianMs']/base['warmMedianMs'])*100
        g['longReductionPercent']=(1-g['longMissMedianMs']/base['longMissMedianMs'])*100
        g['physicalReductionVsDefaultPercent']=(1-g['physicalPeakBytes']/ref['physicalPeakBytes'])*100
        g['cacheReductionVsDefaultPercent']=(1-g['cachePeakBytes']/ref['cachePeakBytes'])*100
        g['primaryPhysicalReductionVsDefaultPercent']=(1-g['physicalPrimaryPeakBytes']/ref['physicalPrimaryPeakBytes'])*100
        g['eligible']=(g['warmReductionPercent']>0 and g['longReductionPercent']>0 and max(g['warmReductionPercent'],g['longReductionPercent'])>=5 and g['physicalReductionVsDefaultPercent']>0 and g['primaryPhysicalReductionVsDefaultPercent']>=5 and g['cacheReductionVsDefaultPercent']>=25)
        if g['eligible']:eligible.append(budget)
    selected=min(eligible,key=lambda b:groups[str(b)]['longMissMedianMs']/groups[str(b)]['pairedControl']['longMissMedianMs']) if eligible else None
    return dict(workloadKind='micro' if micro else 'diverse-history',selectionRules=dict(bothWarmAndLongMediansLower=True,atLeastOneLatencyReductionPercent=5,primaryPhysicalReductionPercent=5,allPathPhysicalMustRemainBelowDefault=True,freeCacheReductionPercent=25,maximumOneMinuteHostLoad=None if micro else 10,maximumRequestAverageBackgroundCpuPercent=50 if micro else None,sustainedBackgroundCpuThresholdPercent=75 if micro else None,sustainedHighSeconds=1 if micro else None),passed=True,scope='qualification pilot comparison; not startup evidence',selectedMiB=selected,selectionPassed=selected is not None,groups=groups,runs=runs,sha256={str(p):hashlib.sha256(Path(p).read_bytes()).hexdigest() for p in paths},limitations=['Two bracketed repeats per candidate, with per-block 256 MiB controls and backend-default references.','Python pilot exact counter + direct backend latency, not shared Rust API latency.','Host activity/thermal state can still vary below the load guard; this is not randomized statistical certification.'])


if __name__=='__main__':
    output=Path(sys.argv[1]);assert not output.exists()
    result=compare(sys.argv[2:])
    with output.open('x') as file:
        output.chmod(0o600);json.dump(result,file,indent=2);file.write('\n')
    print(json.dumps(result,indent=2))
