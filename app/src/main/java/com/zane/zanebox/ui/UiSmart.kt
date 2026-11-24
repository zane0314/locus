@file:OptIn(androidx.compose.foundation.layout.ExperimentalLayoutApi::class)
package com.zane.zanebox.ui

import androidx.compose.animation.core.spring
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.waitForUpOrCancellation
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.zane.zanebox.R
import com.zane.zanebox.config.smartTarget
import com.zane.zanebox.data.AppData
import com.zane.zanebox.subscription.SubscriptionClient
import kotlinx.coroutines.launch
import org.json.JSONArray

internal fun smartDomainRule(line:String):Pair<String,String>? {
    val text=line.trim()
    if(text.isBlank() || text.startsWith('#') || text.startsWith("//"))return null
    val fields=text.split(',').map{it.trim()}
    return when {
        fields.size>=2 && fields[0].uppercase() in listOf("DOMAIN","DOMAIN-SUFFIX","DOMAIN-KEYWORD")->fields[0].uppercase() to fields[1]
        fields.size==1 && text.matches(Regex("[\\p{L}\\p{N}*_.-]+"))->"DOMAIN-SUFFIX" to text
        else->null
    }
}

private const val KaringRuleCatalogUrl =
    "https://api.github.com/repos/KaringX/karing-ruleset/contents/ACL4SSR?ref=sing"

private data class RuleCatalogEntry(val name:String,val url:String,val page:String)

@Composable internal fun SmartPanel(data:AppData,vm:AppViewModel,form:(String,List<Pair<String,String>>,(List<String>)->Unit)->Unit,
    onRules:()->Unit,onMerges:()->Unit,onOpen:(String)->Unit,modifier:Modifier=Modifier) {
    var target by remember{mutableStateOf("")}
    var detailKey by remember{mutableStateOf("")}
    val builtIn=remember{listOf("speed" to "Speed · IP / DNS / 测速","youtube" to "YouTube","telegram" to "Telegram","netflix" to "Netflix","disney" to "Disney+","tiktok" to "TikTok","x" to "X","meta" to "Instagram / Facebook","spotify" to "Spotify","google" to "Google","ai" to "AI 应用")}
    val testing by vm.service.testingNodes.collectAsStateWithLifecycle()
    val summary=remember(data,testing){smartPanelSummary(data,testing)}
    val policyOrder=summary.policyOrder
    val custom=remember(data.settings){data.settings.filterKeys{it.startsWith("smartCustom.") && it.endsWith(".name")}.map{it.key.removePrefix("smartCustom.").removeSuffix(".name") to it.value}}
    val customKeys=remember(custom){custom.map{it.first}.toSet()}
    val policies=remember(builtIn,custom,data.settings){(builtIn+(if(data.settings.containsKey("smartRules.custom") || data.settings.containsKey("smart.custom.target"))listOf("custom" to "自定义规则")else emptyList())+custom).toMap()}
    val ruleCounts=remember(data.rules){data.rules.count{it.prioritize} to data.rules.count{!it.prioritize}}
    val language=LocalUiLanguage.current
    val targetLabels=remember(summary.policies,data.nodes,data.groups,data.merges,language) {
        summary.policies.mapValues{(_,row)->targetName(row.target,data,language)}
    }
    val listState=rememberLazyListState()
    val dragKeys=remember(policyOrder){policyOrder.map{"smart-policy-$it"}}
    val drag=rememberDragSort(dragKeys,listState){keys->vm.reorderSmartPolicies(keys.map{it.toString().removePrefix("smart-policy-")})}
    LazyColumn(modifier.fillMaxSize().testTag("page_list"),state=listState,contentPadding=PaddingValues(start=16.dp,end=16.dp,top=4.dp,bottom=16.dp),verticalArrangement=Arrangement.spacedBy(8.dp)) {
        item(key="smart-rules-section",contentType="smart-section"){UiSection("路由规则")}
        item(key="smart-rules",contentType="smart-card"){UiCard{UiRow("路由规则",uiText("前置 %s · 后置 %s",ruleCounts.first,ruleCounts.second),Icons.Outlined.AccountTree,iconRes=R.drawable.zb_ref_ic_mingcute_route_24,onClick=onRules,modifier=Modifier.testTag("smart_rules"))}}
        item(key="smart-policies-section",contentType="smart-section"){UiSection("应用策略",trailing=uiText("自动选择 · %s 个节点",summary.autoNodeCount))}
        drag.order.forEach { itemKey->
            val key=itemKey.toString().removePrefix("smart-policy-")
            val title=policies[key] ?: return@forEach
            item(key=itemKey,contentType="smart-policy") {
                val row=summary.policies[key]
                val value=row?.target ?: "proxy"
                val node=row?.lowestNode
                val label=targetLabels[key] ?: targetLabel(value,data)
                UiCard(Modifier.animateItem(placementSpec=if(drag.dragging==itemKey)null else spring()).then(drag.modifier(itemKey)).testTag("smart_card_$key")) {
                    SmartPolicyRow(
                        title=if(key in customKeys)title else uiText(title),
                        icon=when(key){"speed"->Icons.Outlined.Speed;"youtube"->Icons.Outlined.PlayCircle;"telegram"->Icons.Outlined.Send;"spotify","tiktok"->Icons.Outlined.MusicNote;"google"->Icons.Outlined.Public;"ai"->Icons.Outlined.AutoAwesome;else->Icons.Outlined.Apps},
                        node=node,
                        targetName=label,
                        latency=row?.latencyLabel,
                        lowestPing=row?.lowestPing ?: -1,
                        key=key,
                        onClick={detailKey=key},
                        onLatency={vm.service.testTarget(value,key)},
                        onTarget={target=key},
                    )
                }
            }
        }
        item(key="smart-add-custom",contentType="smart-action"){OutlinedButton(onClick={form("添加自定义应用组",listOf("名称" to "")){v->require(v[0].isNotBlank()){"请输入名称"};require((builtIn+custom).none{it.second==v[0]}){"名称已存在"};vm.task {val key="custom_${vm.store.nextId()}";vm.store.putSetting("smartCustom.$key.name",v[0]);vm.message.value="已保存"}}},modifier=Modifier.fillMaxWidth().testTag("smart_add_custom")){Icon(Icons.Outlined.Add,null);Text(uiText("添加自定义应用组"))}}
        if(data.setting("serviceMode")!="vpn")item(key="smart-vpn-hint",contentType="smart-hint"){Text(uiText("智能应用路由依赖 VPN 模式。"),style=MaterialTheme.typography.bodySmall,color=MaterialTheme.colorScheme.error,modifier=Modifier.testTag("smart_vpn_hint"))}
    }
    if(target.isNotBlank())TargetPicker("分流目标",smartTarget(data,target),data,{target=""},{vm.selectSmartTarget(target,it)},smart=true)
    if(detailKey.isNotBlank()) {
        val key=detailKey
        val row=summary.policies[key]
        val value=row?.target ?: smartTarget(data,key)
        val label=targetLabels[key] ?: targetName(value,data,language)
        val rawTitle=policies[key] ?: key
        SmartPolicyDetailPage(
            serviceKey=key,
            policyTitle=rawTitle,
            isCustom=key in customKeys,
            targetLabel=label,
            lowestNode=row?.lowestNode,
            existingNames=(builtIn+custom).filter{it.first!=key}.map{it.second}.toSet(),
            data=data,
            vm=vm,
            form=form,
            onDismiss={detailKey=""},
        )
    }
}

/** Node-aligned compact policy card: 48dp icon column, 14sp title + semantic latency, 12sp target/node line. */
@Composable private fun SmartPolicyRow(
    title:String,
    icon:androidx.compose.ui.graphics.vector.ImageVector,
    node:String?,
    targetName:String,
    latency:String?,
    lowestPing:Int,
    key:String,
    onClick:()->Unit,
    onLatency:()->Unit,
    onTarget:()->Unit,
) {
    val latestClick by rememberUpdatedState(onClick)
    Row(
        Modifier.fillMaxWidth().clickable(onClick=onClick).testTag("smart_$key").heightIn(min=60.dp).padding(start=8.dp,end=12.dp),
        verticalAlignment=Alignment.CenterVertically,
    ) {
        Box(Modifier.width(48.dp),contentAlignment=Alignment.Center) {
            Icon(icon,null,Modifier.size(22.dp),tint=MaterialTheme.colorScheme.onSurfaceVariant)
        }
        Column(Modifier.weight(1f)) {
            Box(
                Modifier.fillMaxWidth()
                    .pointerInput(key) {
                        awaitEachGesture {
                            awaitFirstDown(requireUnconsumed=false)
                            val up=waitForUpOrCancellation()
                            if(up!=null && !up.isConsumed) {
                                up.consume()
                                latestClick()
                            }
                        }
                    }
                    .padding(top=10.dp,bottom=2.dp),
            ) {
                TitleWithLatency(
                    title={Text(title,fontSize=14.sp,lineHeight=20.sp,fontWeight=FontWeight.Medium,maxLines=1,overflow=TextOverflow.Ellipsis)},
                    latency=latency?.let { label -> {
                        val color=latencyColor(lowestPing,testing=label=="测试中",failed=label=="失败")
                        val mono=lowestPing>0 || skinPalette.monoNumbers
                        Box(Modifier.testTag("smart_latency_$key").clickable(onClick=onLatency).padding(start=8.dp,end=4.dp,top=2.dp,bottom=2.dp)) {
                            LatencyText(uiText(label),color=color,mono=mono)
                        }
                    } },
                )
            }
            TargetWithNodeLine(
                targetName=targetName,
                node=node,
                key=key,
                onClick=onTarget,
            )
        }
    }
}

/** Second line: target name + optional " → lowestNode", full-width clickable without BoxWithConstraints. */
@Composable private fun TargetWithNodeLine(targetName:String,node:String?,key:String,onClick:()->Unit) {
    val secondary=MaterialTheme.colorScheme.onSurfaceVariant
    if(node==null) {
        Box(Modifier.fillMaxWidth().clickable(onClick=onClick).testTag("smart_target_$key").padding(top=2.dp,bottom=10.dp)) {
            Text(targetName,fontSize=12.sp,lineHeight=16.sp,color=secondary,maxLines=1,overflow=TextOverflow.Ellipsis)
        }
        return
    }
    Layout(
        content={
            Text(targetName,fontSize=12.sp,lineHeight=16.sp,color=secondary,maxLines=1,overflow=TextOverflow.Ellipsis)
            Text(" → ",fontSize=12.sp,lineHeight=16.sp,color=secondary,softWrap=false,maxLines=1)
            Text(node,Modifier.testTag("smart_node_$key"),fontSize=12.sp,lineHeight=16.sp,color=secondary,maxLines=1,overflow=TextOverflow.Ellipsis)
        },
        modifier=Modifier.fillMaxWidth().clickable(onClick=onClick).testTag("smart_target_$key").padding(top=2.dp,bottom=10.dp),
    ) { measurables,constraints ->
        val width=constraints.maxWidth
        val arrow=measurables[1].measure(Constraints(maxWidth=width))
        val available=(width-arrow.width).coerceAtLeast(0)
        val targetNatural=measurables[0].maxIntrinsicWidth(Constraints.Infinity)
        val targetCap=minOf(targetNatural,available/2)
        val targetPlaceable=measurables[0].measure(Constraints(maxWidth=targetCap.coerceAtLeast(0)))
        val nodeCap=(available-targetPlaceable.width).coerceAtLeast(0)
        val nodePlaceable=measurables[2].measure(Constraints(maxWidth=nodeCap))
        val height=maxOf(targetPlaceable.height,arrow.height,nodePlaceable.height)
        layout(width,height) {
            targetPlaceable.placeRelative(0,(height-targetPlaceable.height)/2)
            arrow.placeRelative(targetPlaceable.width,(height-arrow.height)/2)
            nodePlaceable.placeRelative(targetPlaceable.width+arrow.width,(height-nodePlaceable.height)/2)
        }
    }
}

/** Keeps latency complete on one line, measured from its finite intrinsic width. */
@Composable private fun LatencyText(value:String,color:Color,mono:Boolean) {
    Layout({
        Text(
            value,
            fontSize=12.sp,
            lineHeight=16.sp,
            fontFamily=if(mono)FontFamily.Monospace else FontFamily.Default,
            color=color,
            softWrap=false,
            maxLines=1,
        )
    }) { measurables,_ ->
        val part=measurables.single().let { it.measure(Constraints.fixedWidth(it.maxIntrinsicWidth(Constraints.Infinity))) }
        layout(part.width,part.height) { part.placeRelative(0,0) }
    }
}

/**
 * Title and latency share a line while the latency keeps its full natural width and the title keeps a readable minimum;
 * otherwise the latency moves under the title, right-aligned.
 */
@Composable private fun TitleWithLatency(title:@Composable ()->Unit,latency:(@Composable ()->Unit)?) {
    if(latency==null) { title();return }
    Layout({ title();latency() },Modifier.fillMaxWidth()) { measurables,constraints ->
        val width=constraints.maxWidth
        val action=measurables[1].measure(Constraints(maxWidth=width))
        val side=width-action.width>=88.dp.roundToPx()
        val head=measurables[0].measure(Constraints(maxWidth=if(side)width-action.width else width))
        if(side) {
            val height=maxOf(head.height,action.height)
            layout(width,height) { head.placeRelative(0,(height-head.height)/2);action.placeRelative(width-action.width,(height-action.height)/2) }
        } else layout(width,head.height+action.height) { head.placeRelative(0,0);action.placeRelative(width-action.width,head.height) }
    }
}

@Composable internal fun SmartMenu(data:AppData,vm:AppViewModel,onOpen:(String)->Unit) {
    var updateChoice by remember{mutableStateOf("")}
    UiMenu(listOf(
        (if(data.setting("routeMode")=="global")"退出全局模式" else "全局模式") to {vm.setting("routeMode",if(data.setting("routeMode")=="global")"rule" else "global")},
        "路由规则" to {onOpen("rules")},
        "规则自动更新间隔" to {updateChoice="rulesUpdateInterval"},
        "连接后检查规则" to {updateChoice="rulesUpdateDelay"},
        "分流检测" to {onOpen("routing-probe")},
        "站点分流卡片" to {onOpen("site-cards")},
        "规则集预览" to {onOpen("ruleset-preview")},
        "域名穿透" to {onOpen("penetration")},
        "Geo 状态" to {onOpen("geo-status")},
        "路由资产" to {onOpen("assets")},
        "流量统计" to {onOpen("traffic")},
        "节点汇总组" to {onOpen("merges")},
        "备份与恢复" to {onOpen("backup")}),"smart_menu",circle=true)
    if(updateChoice.isNotBlank())ChoiceDialog(
        if(updateChoice=="rulesUpdateInterval")"自动更新间隔" else "连接后检查",
        data.setting(updateChoice,if(updateChoice=="rulesUpdateInterval")"24h" else "30s"),
        if(updateChoice=="rulesUpdateInterval")listOf("off" to "关闭","6h" to "6 小时","12h" to "12 小时","24h" to "24 小时","3d" to "3 天","7d" to "7 天")
        else listOf("0s" to "立即","15s" to "15 秒","30s" to "30 秒","1m" to "1 分钟","5m" to "5 分钟"),
        {updateChoice=""},
    ){vm.setting(updateChoice,it)}
}

@Composable
private fun SmartPolicyDetailPage(
    serviceKey:String,
    policyTitle:String,
    isCustom:Boolean,
    targetLabel:String,
    lowestNode:String?,
    existingNames:Set<String>,
    data:AppData,
    vm:AppViewModel,
    form:(String,List<Pair<String,String>>,(List<String>)->Unit)->Unit,
    onDismiss:()->Unit,
) {
    val language=LocalUiLanguage.current
    val installedPackages=rememberInstalledPackageNames()
    val bundled=remember(serviceKey,vm){com.zane.zanebox.config.builtinSmartRuleText(serviceKey){path->vm.getApplication<android.app.Application>().assets.open(path).bufferedReader().use{it.readText()}}}
    val initialUrl=data.setting("smartUrl.$serviceKey")
    val usesBuiltin=serviceKey in com.zane.zanebox.config.builtinSmartRuleFiles && initialUrl.isBlank() && (!data.settings.containsKey("smartRules.$serviceKey") || com.zane.zanebox.config.isBuiltinSmartRuleText(data.setting("smartRules.$serviceKey"),bundled))
    val initialRules=remember(serviceKey,data.settings,bundled) {
        val saved=data.setting("smartRules.$serviceKey")
        if(serviceKey in com.zane.zanebox.config.builtinSmartRuleFiles && data.setting("smartUrl.$serviceKey").isBlank() && (!data.settings.containsKey("smartRules.$serviceKey") || com.zane.zanebox.config.isBuiltinSmartRuleText(saved,bundled)))com.zane.zanebox.config.supportedBuiltinSmartRules(bundled) else saved
    }
    var url by remember(serviceKey,initialUrl){mutableStateOf(initialUrl)}
    var rules by remember(serviceKey,initialRules){mutableStateOf(initialRules)}
    var error by remember{mutableStateOf("")}
    var urlDialogOpen by remember{mutableStateOf(false)}
    var urlDraft by remember{mutableStateOf("")}
    var urlDialogError by remember{mutableStateOf("")}
    var catalogError by remember{mutableStateOf("")}
    var catalog by remember{mutableStateOf<List<RuleCatalogEntry>>(emptyList())}
    var catalogQuery by remember{mutableStateOf("")}
    var catalogOpen by remember{mutableStateOf(false)}
    var catalogLoading by remember{mutableStateOf(false)}
    var fetching by remember{mutableStateOf(false)}
    var confirmDeleteSource by remember{mutableStateOf(false)}
    var confirmDeleteGroup by remember{mutableStateOf(false)}
    var appsOpen by remember{mutableStateOf(false)}
    var rawEditor by remember{mutableStateOf(false)}
    var domainIndex by remember{mutableStateOf<Int?>(null)}
    var domainType by remember{mutableStateOf("DOMAIN-SUFFIX")}
    var domainValue by remember{mutableStateOf("")}
    var typePicker by remember{mutableStateOf(false)}
    var domainInput by remember{mutableStateOf(false)}
    var domainError by remember{mutableStateOf("")}
    val domainTypes=listOf("DOMAIN" to "完整域名 · DOMAIN","DOMAIN-SUFFIX" to "域名后缀 · DOMAIN-SUFFIX","DOMAIN-KEYWORD" to "域名关键词 · DOMAIN-KEYWORD")
    val ruleLines=remember(rules){rules.lines()}
    val visibleRuleIndices=remember(ruleLines){ruleLines.indices.filter{ruleLines[it].isNotBlank() && !ruleLines[it].trim().startsWith('#') && !ruleLines[it].trim().startsWith("//")}}
    val compatibility=remember(serviceKey,rules,language){com.zane.zanebox.config.ConfigBuilder.ruleCompatibilityWarnings(serviceKey,rules){service,type->translateText("%s：此规则格式暂不支持 %s；该类条目保留但不参与匹配，其余有效规则仍生效",language,service,type)}}
    val busy by vm.busy.collectAsStateWithLifecycle()
    val scope=rememberCoroutineScope()

    fun editDomain(index:Int) {
        val match=ruleLines.getOrNull(index)?.let(::smartDomainRule)
        domainIndex=index;domainType=match?.first ?: "DOMAIN-SUFFIX";domainValue=match?.second.orEmpty();domainError="";typePicker=true
    }
    fun validateUrlText(candidate:String):String? {
        if(candidate.isBlank())return null
        return runCatching { validatePreference("testUrl",candidate) }.exceptionOrNull()?.let{it.message ?: "规则源 URL 无效"}
    }
    fun saveBulkRules() {
        error=""
        val savedRules=rules
        vm.edit(onError={error=it}) { d->
            d.copy(settings=d.settings+("smartRules.$serviceKey" to savedRules))
        }
    }
    fun fetchAndReplace() {
        error=""
        val requestUrl=url.trim()
        if(requestUrl.isBlank()){error="请先填写规则源 URL";return}
        validateUrlText(requestUrl)?.let{error=it;return}
        val requestRules=rules
        val baseline=data
        fetching=true
        scope.launch {
            try {
                val fetchedRules=SubscriptionClient.fetchSmartRules(requestUrl,data.settings)
                require(url.trim()==requestUrl && rules==requestRules) { "规则来源或编辑内容已变更，已保留当前规则，请重新获取" }
                vm.edit(onSaved={rules=fetchedRules},onError={error=it}) { d->
                    com.zane.zanebox.config.applySmartRuleUpdate(d,serviceKey,baseline,fetchedRules,sourceUrl=requestUrl)
                }
            } catch(e:Exception) {
                error=e.message ?: "规则源获取失败"
            } finally { fetching=false }
        }
    }
    fun loadCatalog() {
        catalogOpen=true
        if(catalog.isNotEmpty() || catalogLoading)return
        catalogError=""
        catalogLoading=true
        scope.launch {
            try {
                val body=SubscriptionClient.fetch(KaringRuleCatalogUrl,settings=data.settings).body
                val json=JSONArray(body)
                catalog=(0 until json.length()).mapNotNull { index ->
                    val entry=json.optJSONObject(index) ?: return@mapNotNull null
                    if(entry.optString("type")!="file")return@mapNotNull null
                    val name=entry.optString("name").trim()
                    val download=entry.optString("download_url").trim()
                    if(name.isBlank() || download.isBlank())null else RuleCatalogEntry(name,download,entry.optString("html_url"))
                }.sortedBy{it.name.lowercase()}
                if(catalog.isEmpty())catalogError="目录没有可用规则文件"
            } catch(e:Exception) {
                catalogError=e.message ?: "Karing 目录加载失败"
            } finally { catalogLoading=false }
        }
    }
    val filteredCatalog=remember(catalog,catalogQuery){
        val query=catalogQuery.trim().lowercase()
        if(query.isBlank())catalog else catalog.filter{it.name.lowercase().contains(query) || it.url.lowercase().contains(query)}
    }
    val detailMenuActions=buildList {
        add("删除规则源" to {confirmDeleteSource=true})
        if(isCustom) {
            add("重命名" to {form("重命名",listOf("名称" to policyTitle)){values->
                require(values[0].isNotBlank())
                require(values[0] !in existingNames){"名称已存在"}
                vm.setting("smartCustom.$serviceKey.name",values[0])
            }})
            add("删除应用组" to {confirmDeleteGroup=true})
        }
    }

    if(appsOpen) {
        AppsEditor(
            com.zane.zanebox.config.appPackages(data.setting("smartCustom.$serviceKey.packages")),
            {appsOpen=false},
            scopeData=data,
            title=uiText("选择应用 · %s",data.setting("smartCustom.$serviceKey.name").ifBlank{serviceKey}),
        ) { packages->
            vm.edit(onSaved={appsOpen=false},autoApply=false){it.copy(settings=it.settings+("smartCustom.$serviceKey.packages" to packages.sorted().joinToString("\n")))}
        }
        return
    }

    UiPageList(policyTitle,onDismiss,translateTitle=!isCustom,itemSpacing=0.dp,action={
        UiMenu(detailMenuActions,"smart_detail_menu")
    }) {
        item(key="detail-summary",contentType="detail-summary") {
            UiCard {
                Box(Modifier.fillMaxWidth().padding(horizontal=16.dp,vertical=12.dp)) {
                    if(lowestNode!=null) {
                        FlowRow(horizontalArrangement=Arrangement.spacedBy(6.dp),verticalArrangement=Arrangement.spacedBy(2.dp)) {
                            Box(Modifier.testTag("smart_goal_$serviceKey")) {
                                Text(targetLabel,fontSize=13.sp,lineHeight=18.sp,fontWeight=FontWeight.Medium,color=MaterialTheme.colorScheme.onSurface)
                            }
                            Text("→",fontSize=13.sp,lineHeight=18.sp,color=MaterialTheme.colorScheme.onSurfaceVariant)
                            Box(Modifier.testTag("smart_current_$serviceKey")) {
                                Text(lowestNode,fontSize=13.sp,lineHeight=18.sp,color=MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                        }
                    } else {
                        Box(Modifier.testTag("smart_goal_$serviceKey")) {
                            Text(targetLabel,fontSize=13.sp,lineHeight=18.sp,fontWeight=FontWeight.Medium,color=MaterialTheme.colorScheme.onSurface)
                        }
                    }
                }
            }
        }
        item(key="detail-apps-section",contentType="detail-section") {
            Spacer(Modifier.height(6.dp))
            UiSection("应用分流")
        }
        item(key="detail-apps",contentType="detail-card") {
            UiCard {
                UiRow(
                    "选择应用",
                    if(installedPackages==null)uiText("正在读取应用") else uiText("%s 个有效应用",com.zane.zanebox.config.effectiveSmartPackages(data,serviceKey,installedPackages).size),
                    Icons.Outlined.Apps,
                    minHeight=56.dp,
                    onClick={appsOpen=true},
                    modifier=Modifier.testTag("smart_apps_$serviceKey"),
                )
            }
        }
        item(key="detail-rules-section",contentType="detail-section") {
            Spacer(Modifier.height(6.dp))
            UiSection("分流规则")
        }
        item(key="detail-rule-source",contentType="detail-card") {
            UiCard {
                UiRow(
                    "规则来源",
                    url.ifBlank{uiText(if(usesBuiltin)"内置兼容规则组" else "自定义域名规则")},
                    Icons.Outlined.Description,
                    minHeight=56.dp,
                    translateSubtitle=false,
                    onClick={urlDraft=url;urlDialogError="";urlDialogOpen=true},
                    modifier=Modifier.testTag("rule_source_row"),
                    trailing={
                        Row(verticalAlignment=Alignment.CenterVertically) {
                            IconButton(onClick=::fetchAndReplace,enabled=!busy&&!fetching,modifier=Modifier.testTag("rule_source_replace")) {
                                if(fetching)CircularProgressIndicator(Modifier.size(22.dp),strokeWidth=2.dp)
                                else Icon(Icons.Outlined.Download,uiText("获取并替换"),Modifier.size(20.dp),tint=skinSecondaryText)
                            }
                            Icon(Icons.Outlined.ChevronRight,null,Modifier.size(18.dp),tint=MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    },
                )
                if(compatibility.isNotEmpty()) {
                    Text(
                        compatibility.joinToString("\n"),
                        fontSize=12.sp,
                        lineHeight=16.sp,
                        color=skinWarning,
                        modifier=Modifier.padding(start=50.dp,end=16.dp,bottom=if(error.isNotBlank())4.dp else 10.dp).testTag("rule_source_compatibility"),
                    )
                }
                if(error.isNotBlank()) {
                    Text(
                        uiText(error),
                        fontSize=12.sp,
                        lineHeight=16.sp,
                        color=MaterialTheme.colorScheme.error,
                        modifier=Modifier.padding(start=50.dp,end=16.dp,bottom=10.dp).testTag("rule_source_error"),
                    )
                }
            }
        }
        item(key="detail-custom-section",contentType="detail-section") {
            Spacer(Modifier.height(6.dp))
            UiSection(
                uiText("自定义规则 · %s 条",visibleRuleIndices.size),
                translateTitle=false,
                action={
                    TextButton(
                        onClick={editDomain(-1)},
                        contentPadding=PaddingValues(horizontal=8.dp,vertical=0.dp),
                        modifier=Modifier.heightIn(min=32.dp).testTag("rule_domain_add"),
                    ) {
                        Icon(Icons.Outlined.Add,null,Modifier.size(16.dp))
                        Spacer(Modifier.width(2.dp))
                        Text(uiText("添加"),fontSize=12.sp)
                    }
                },
            )
        }
        if(rawEditor) {
            item(key="rule-raw-editor",contentType="rule-raw-editor") {
                UiCard {
                    UiRow("批量编辑文本","收起批量编辑",Icons.Outlined.Code,minHeight=48.dp,onClick={rawEditor=false},modifier=Modifier.testTag("rule_source_raw"))
                    HorizontalDivider(Modifier.padding(start=50.dp,end=16.dp),thickness=.5.dp,color=MaterialTheme.colorScheme.outlineVariant.copy(alpha=.4f))
                    Column(Modifier.padding(16.dp),verticalArrangement=Arrangement.spacedBy(10.dp)) {
                        OutlinedTextField(
                            rules,
                            {rules=it;error=""},
                            label={Text(uiText("自定义规则（每行域名或 DOMAIN / IP-CIDR）"))},
                            modifier=Modifier.fillMaxWidth().testTag("rule_source_rules"),
                            minLines=8,
                            maxLines=16,
                        )
                        Row(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.End) {
                            Button(onClick=::saveBulkRules,enabled=!busy&&!fetching,modifier=Modifier.testTag("rule_source_save")) {
                                Text(uiText("保存"))
                            }
                        }
                    }
                }
            }
        } else {
            itemsIndexed(visibleRuleIndices,key={_,index->"rule-row-$index"},contentType={_,_->"rule-row"}) { pos,index ->
                val line=ruleLines[index]
                val match=smartDomainRule(line)
                val badge=when(match?.first) {
                    "DOMAIN"->"完整"
                    "DOMAIN-SUFFIX"->"后缀"
                    "DOMAIN-KEYWORD"->"关键词"
                    else->"原始"
                }
                UiGroupedCardSlice(first=pos==0,last=false) {
                    Row(
                        Modifier.fillMaxWidth()
                            .clickable(onClick=if(match!=null)({editDomain(index)})else ({rawEditor=true}))
                            .testTag("rule_domain_$index")
                            .heightIn(min=48.dp)
                            .padding(horizontal=16.dp,vertical=10.dp),
                        verticalAlignment=Alignment.CenterVertically,
                    ) {
                        Text(match?.second ?: line,Modifier.weight(1f),fontSize=14.sp,lineHeight=20.sp,fontWeight=FontWeight.Medium,maxLines=1,overflow=TextOverflow.Ellipsis)
                        Spacer(Modifier.width(8.dp))
                        Text(uiText(badge),fontSize=12.sp,lineHeight=16.sp,color=MaterialTheme.colorScheme.onSurfaceVariant)
                        Spacer(Modifier.width(4.dp))
                        Icon(Icons.Outlined.ChevronRight,null,Modifier.size(18.dp),tint=MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                    HorizontalDivider(Modifier.padding(horizontal=16.dp),thickness=.5.dp,color=MaterialTheme.colorScheme.outlineVariant.copy(alpha=.4f))
                }
            }
            item(key="rule-source-raw",contentType="rule-raw") {
                UiGroupedCardSlice(first=visibleRuleIndices.isEmpty(),last=true) {
                    UiRow("批量编辑文本",minHeight=48.dp,onClick={rawEditor=true},modifier=Modifier.testTag("rule_source_raw"))
                }
            }
        }
    }

    if(urlDialogOpen)UiAlertDialog(
        onDismissRequest={urlDialogOpen=false},
        title={Text(uiText("规则来源"))},
        text={
            Column(verticalArrangement=Arrangement.spacedBy(10.dp)) {
                OutlinedTextField(
                    urlDraft,
                    {urlDraft=it;urlDialogError=""},
                    label={Text(uiText("规则源 URL"))},
                    singleLine=true,
                    modifier=Modifier.fillMaxWidth().testTag("rule_source_url"),
                )
                OutlinedButton(onClick=::loadCatalog,enabled=!busy&&!fetching,modifier=Modifier.fillMaxWidth().testTag("rule_catalog")) {
                    Icon(Icons.Outlined.Search,null);Spacer(Modifier.width(6.dp));Text(uiText("在线目录 / Karing 搜索"))
                }
                if(urlDialogError.isNotBlank())Text(uiText(urlDialogError),fontSize=12.sp,color=MaterialTheme.colorScheme.error)
            }
        },
        confirmButton={
            TextButton(
                onClick={
                    val savedUrl=urlDraft.trim()
                    val validationError=validateUrlText(savedUrl)
                    if(validationError!=null) {
                        urlDialogError=validationError
                    } else {
                        urlDialogError=""
                        vm.edit(
                            onSaved={url=savedUrl;urlDialogOpen=false},
                            onError={urlDialogError=it},
                        ) { d->
                            d.copy(settings=d.settings+("smartUrl.$serviceKey" to savedUrl))
                        }
                    }
                },
                enabled=!busy&&!fetching,
                modifier=Modifier.testTag("rule_source_url_confirm"),
            ){Text(uiText("确定"))}
        },
        dismissButton={TextButton(onClick={urlDialogOpen=false}){Text(uiText("取消"))}},
    )

    if(typePicker)ChoiceDialog("域名匹配类型",domainType,domainTypes,{typePicker=false}){domainType=it;typePicker=false;domainInput=true}
    if(domainInput)UiAlertDialog(
        onDismissRequest={domainInput=false},
        title={Text(uiText(if(domainIndex==-1)"添加域名规则" else "编辑域名规则"))},
        text={
            Column(verticalArrangement=Arrangement.spacedBy(10.dp)) {
                UiRow("匹配类型",domainTypes.first{it.first==domainType}.second,onClick={typePicker=true},modifier=Modifier.testTag("rule_domain_type"))
                OutlinedTextField(domainValue,{domainValue=it;domainError=""},label={Text(uiText(if(domainType=="DOMAIN-KEYWORD")"关键词" else "域名"))},singleLine=true,modifier=Modifier.fillMaxWidth().testTag("rule_domain_value"))
                if(domainError.isNotBlank())Text(uiText(domainError),color=MaterialTheme.colorScheme.error)
            }
        },
        confirmButton={
            TextButton(onClick={
                val value=domainValue.trim()
                if(value.isBlank() || value.any{it==',' || it.isWhitespace()})domainError="请输入单个域名或关键词，不需要填写规则前缀"
                else {
                    val index=domainIndex ?: -1
                    val lines=ruleLines.toMutableList()
                    val tail=lines.getOrNull(index)?.split(',')?.drop(2).orEmpty()
                    val line=(listOf(domainType,value)+tail).joinToString(",")
                    if(index in lines.indices)lines[index]=line else lines.add(line)
                    val nextRules=lines.joinToString("\n")
                    vm.edit(
                        onSaved={rules=nextRules;error="";domainInput=false},
                        onError={domainError=it},
                    ) { d->
                        d.copy(settings=d.settings+("smartRules.$serviceKey" to nextRules))
                    }
                }
            },enabled=!busy,modifier=Modifier.testTag("rule_domain_confirm")){Text(uiText(if(domainIndex==-1)"添加" else "替换"))}
        },
        dismissButton={TextButton(onClick={domainInput=false}){Text(uiText("取消"))}},
    )

    if(catalogOpen)UiAlertDialog(
        onDismissRequest={catalogOpen=false},
        title={Text(uiText("Karing 在线规则目录"))},
        text={
            Column(verticalArrangement=Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(catalogQuery,{catalogQuery=it},label={Text(uiText("搜索 Karing 规则"))},singleLine=true,
                    modifier=Modifier.fillMaxWidth().testTag("rule_catalog_search"))
                if(catalogLoading)Row(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.Center){CircularProgressIndicator(Modifier.size(28.dp))}
                if(catalogError.isNotBlank())Text(uiText(catalogError),color=MaterialTheme.colorScheme.error,modifier=Modifier.testTag("rule_catalog_error"))
                LazyColumn(Modifier.heightIn(max=420.dp)) {
                    items(filteredCatalog,key={it.url}) { entry ->
                        Row(Modifier.fillMaxWidth().padding(vertical=4.dp).testTag("rule_catalog_item_${entry.name}"),verticalAlignment=Alignment.CenterVertically) {
                            Column(Modifier.weight(1f).clickable{urlDraft=entry.url;catalogOpen=false;urlDialogError=""}) {
                                Text(entry.name,maxLines=1)
                                if(entry.page.isNotBlank())Text(entry.page,color=MaterialTheme.colorScheme.onSurfaceVariant,style=MaterialTheme.typography.labelSmall,maxLines=1)
                            }
                            TextButton(onClick={urlDraft=entry.url;catalogOpen=false;urlDialogError=""},modifier=Modifier.testTag("rule_catalog_replace_${entry.name}")){Text(uiText("替换"))}
                        }
                    }
                }
                if(!catalogLoading && catalog.isNotEmpty() && filteredCatalog.isEmpty())Text(uiText("没有匹配的 Karing 规则"),modifier=Modifier.testTag("rule_catalog_empty"))
            }
        },
        confirmButton={TextButton(onClick={catalogOpen=false}){Text(uiText("关闭"))}},
    )

    if(confirmDeleteSource)UiAlertDialog(
        onDismissRequest={confirmDeleteSource=false},
        title={Text(uiText("删除规则源？"))},
        text={Text(uiText("删除后将清除该应用的 URL 与已保存规则。"))},
        confirmButton={
            TextButton(onClick={
                confirmDeleteSource=false
                vm.edit(onSaved={url="";rules="";rawEditor=false;error=""},onError={error=it}) { d->
                    d.copy(settings=d.settings.filterKeys { key -> key!="smartUrl.$serviceKey" && key!="smartUpdated.$serviceKey" }+("smartRules.$serviceKey" to ""))
                }
            },enabled=!busy,modifier=Modifier.testTag("rule_source_delete_confirm")){Text(uiText("删除"))}
        },
        dismissButton={TextButton(onClick={confirmDeleteSource=false}){Text(uiText("取消"))}},
    )

    if(confirmDeleteGroup)UiAlertDialog(
        onDismissRequest={confirmDeleteGroup=false},
        title={Text(uiText("删除自定义应用组？"))},
        confirmButton={
            TextButton(onClick={
                confirmDeleteGroup=false
                vm.edit(onSaved=onDismiss) { d->
                    d.copy(settings=d.settings.filterKeys{!it.startsWith("smartCustom.$serviceKey.") && it!="smart.$serviceKey.target" && it!="smartUrl.$serviceKey" && it!="smartRules.$serviceKey"})
                }
            },enabled=!busy){Text(uiText("删除"))}
        },
        dismissButton={TextButton(onClick={confirmDeleteGroup=false}){Text(uiText("取消"))}},
    )
}
