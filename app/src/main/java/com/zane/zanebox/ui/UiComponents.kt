@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class,androidx.compose.ui.ExperimentalComposeUiApi::class,androidx.compose.foundation.ExperimentalFoundationApi::class)
package com.zane.zanebox.ui

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.painter.Painter
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.*
import androidx.compose.ui.semantics.testTagsAsResourceId
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties

internal val LocalUiSnackbar=staticCompositionLocalOf<SnackbarHostState?> { null }
internal val LocalUiBusy=staticCompositionLocalOf { false }
internal val LocalUiListState=staticCompositionLocalOf<androidx.compose.foundation.lazy.LazyListState?> { null }
internal val LocalUiReload=staticCompositionLocalOf<(() -> Unit)?> { null }

@OptIn(ExperimentalLayoutApi::class)
@Composable internal fun NodeSelectionBar(selecting:Boolean,count:Int,tag:String,onMode:()->Unit,onAll:()->Unit,onDelete:()->Unit) {
    FlowRow(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.End) {
        TextButton(onClick=onMode,modifier=Modifier.testTag(if(selecting)"${tag}_selection_cancel" else "${tag}_multiselect")){Text(uiText(if(selecting)"取消多选" else "多选"))}
        if(selecting) {
            TextButton(onClick=onAll,modifier=Modifier.testTag("${tag}_select_all")){Text(uiText("全选"))}
            TextButton(onClick=onDelete,enabled=count>0,modifier=Modifier.testTag("${tag}_delete_selected")){Text(uiText("删除 (%s)",count))}
        }
    }
}

@Composable internal fun ApplyChangesRow(onReload:()->Unit,modifier:Modifier=Modifier) {
    Surface(color=MaterialTheme.colorScheme.surface) {
        Row(modifier.fillMaxWidth().clickable(onClick=onReload).padding(horizontal=16.dp,vertical=8.dp),verticalAlignment=Alignment.CenterVertically) {
            Text(uiText("重载代理服务以应用修改"),Modifier.weight(1f),fontSize=14.sp)
            Spacer(Modifier.width(12.dp))
            Button(onClick=onReload,modifier=Modifier.testTag("apply_changes")){Text(uiText("重启服务"))}
        }
    }
}

/** Pure Compose M3-styled switch inside an exact 48x36dp accessible target. */
@Composable internal fun UiSwitch(checked:Boolean,onCheckedChange:((Boolean)->Unit)?,modifier:Modifier=Modifier,enabled:Boolean=true) {
    val primary=MaterialTheme.colorScheme.primary
    val dark=MaterialTheme.colorScheme.onSurface.luminance()>.5f
    val targetTrack=when {
        checked && enabled -> primary
        checked -> primary.copy(alpha=.45f)
        dark -> androidx.compose.ui.graphics.Color(0xFF2E3138)
        else -> androidx.compose.ui.graphics.Color(0xFFD5D8DF)
    }
    val targetThumb=when {
        checked -> MaterialTheme.colorScheme.onPrimary
        dark -> androidx.compose.ui.graphics.Color(0xFFA0A5B1)
        else -> androidx.compose.ui.graphics.Color.White
    }
    val trackColor by animateColorAsState(targetTrack,tween(160),label="switchTrack")
    val thumbColor by animateColorAsState(targetThumb,tween(160),label="switchThumb")
    val thumbOffset by animateDpAsState(if(checked)20.dp else 3.dp,tween(160),label="switchOffset")
    Box(
        modifier.size(width=48.dp,height=36.dp).then(
            if(onCheckedChange==null)Modifier.semantics {
                role=Role.Switch
                toggleableState=if(checked)androidx.compose.ui.state.ToggleableState.On else androidx.compose.ui.state.ToggleableState.Off
            } else Modifier.toggleable(checked,enabled=enabled,role=Role.Switch,onValueChange=onCheckedChange)
        ),
        contentAlignment=Alignment.Center,
    ) {
        Box(
            Modifier.size(width=40.dp,height=22.dp)
                .background(trackColor,RoundedCornerShape(11.dp)),
            contentAlignment=Alignment.CenterStart,
        ) {
            Box(
                Modifier.offset(x=thumbOffset).size(16.dp)
                    .background(thumbColor,CircleShape)
            )
        }
    }
}

@Composable internal fun UiCard(modifier:Modifier=Modifier,selected:Boolean=false, content:@Composable ColumnScope.()->Unit) {
    val linear=skinPalette.name!="prism"
    val borderColor=when {
        selected && linear -> skinPrimary.copy(alpha=.7f)
        linear -> skinDivider
        else -> MaterialTheme.colorScheme.outlineVariant.copy(alpha=.5f)
    }
    val borderWidth=if(linear)1.dp else .5.dp
    Surface(modifier.fillMaxWidth(),shape=MaterialTheme.shapes.large,
        color=if(selected)skinSelectedSurface else MaterialTheme.colorScheme.surface,
        border=BorderStroke(borderWidth,borderColor)) {
        Column(content=content)
    }
}

/** Lazy-friendly slice of a grouped card so large rule lists share one card outline without eager composition. */
@Composable internal fun UiGroupedCardSlice(first:Boolean,last:Boolean,modifier:Modifier=Modifier,content:@Composable ColumnScope.()->Unit) {
    if(first && last) { UiCard(modifier,content=content); return }
    val linear=skinPalette.name!="prism"
    val bg=MaterialTheme.colorScheme.surface
    val borderColor=if(linear)skinDivider else MaterialTheme.colorScheme.outlineVariant.copy(alpha=.5f)
    val borderWidth=if(linear)1.dp else .5.dp
    val radius=when(skinPalette.name) { "miro"->24.dp; "prism"->14.dp; else->12.dp }
    val shape=when {
        first -> RoundedCornerShape(topStart=radius,topEnd=radius)
        last -> RoundedCornerShape(bottomStart=radius,bottomEnd=radius)
        else -> RectangleShape
    }
    Column(
        modifier.fillMaxWidth().clip(shape).background(bg).drawWithContent {
            drawContent()
            val w=borderWidth.toPx()
            val half=w/2f
            val r=minOf(radius.toPx(),size.width/2f,size.height/2f)
            when {
                first -> {
                    val path=Path().apply {
                        moveTo(half,size.height)
                        lineTo(half,r)
                        arcTo(Rect(half,half,2*r-half,2*r-half),180f,90f,false)
                        lineTo(size.width-r,half)
                        arcTo(Rect(size.width-2*r+half,half,size.width-half,2*r-half),270f,90f,false)
                        lineTo(size.width-half,size.height)
                    }
                    drawPath(path,borderColor,style=Stroke(width=w))
                }
                last -> {
                    val path=Path().apply {
                        moveTo(size.width-half,0f)
                        lineTo(size.width-half,size.height-r)
                        arcTo(Rect(size.width-2*r+half,size.height-2*r+half,size.width-half,size.height-half),0f,90f,false)
                        lineTo(r,size.height-half)
                        arcTo(Rect(half,size.height-2*r+half,2*r-half,size.height-half),90f,90f,false)
                        lineTo(half,0f)
                    }
                    drawPath(path,borderColor,style=Stroke(width=w))
                }
                else -> {
                    drawLine(borderColor,Offset(half,0f),Offset(half,size.height),strokeWidth=w)
                    drawLine(borderColor,Offset(size.width-half,0f),Offset(size.width-half,size.height),strokeWidth=w)
                }
            }
        },
        content=content,
    )
}

@Composable internal fun latencyColor(ping:Int,testing:Boolean=false,failed:Boolean=false):Color = when {
    testing -> skinPrimaryDark
    failed || ping==-2 -> skinError.copy(alpha=.85f)
    ping in 1..150 -> skinSuccess
    ping in 151..350 -> skinWarning
    ping>350 -> skinError
    else -> skinPalette.textTertiary
}

@Composable private fun CompactActionShell(tag:String,onClick:()->Unit,content:@Composable ()->Unit) {
    if(skinPalette.name=="prism") {
        FilledIconButton(onClick=onClick,modifier=Modifier.size(48.dp).testTag(tag),colors=IconButtonDefaults.filledIconButtonColors(containerColor=Color.White,contentColor=skinOnSurface)) { content() }
    } else {
        Box(Modifier.size(48.dp).clickable(onClick=onClick).testTag(tag),contentAlignment=Alignment.Center) {
            Surface(Modifier.size(38.dp),shape=RoundedCornerShape(8.dp),color=skinSurfaceElevated,border=BorderStroke(1.dp,skinDivider)) {
                Box(contentAlignment=Alignment.Center) { content() }
            }
        }
    }
}

@Composable internal fun CompactActionButton(icon:ImageVector,description:String,tag:String,onClick:()->Unit,iconSize:Dp=19.dp) {
    CompactActionShell(tag,onClick) {
        if(skinPalette.name=="prism")Icon(icon,description)
        else Icon(icon,description,Modifier.size(iconSize),tint=skinOnSurface)
    }
}

@Composable internal fun CompactActionButton(painter:Painter,description:String,tag:String,onClick:()->Unit,iconSize:Dp=19.dp) {
    CompactActionShell(tag,onClick) {
        if(skinPalette.name=="prism")Icon(painter,description)
        else Icon(painter,description,Modifier.size(iconSize),tint=skinOnSurface)
    }
}

@Composable internal fun UiRow(title:String,subtitle:String="",icon:ImageVector?=null,
    modifier:Modifier=Modifier,onClick:(()->Unit)?=null,trailing:(@Composable ()->Unit)?=null,iconRes:Int=0,minHeight:Dp=62.dp,titleSize:androidx.compose.ui.unit.TextUnit=14.sp,chevron:Boolean=true,reserveIcon:Boolean=false,summaryLines:Int=1,onLongClick:(()->Unit)?=null,translateTitle:Boolean=true,translateSubtitle:Boolean=true) {
    Row(modifier.fillMaxWidth().then(if(onLongClick!=null)Modifier.combinedClickable(onClick=onClick ?: {},onLongClick=onLongClick) else if(onClick==null) Modifier else Modifier.clickable(onClick=onClick))
        .heightIn(min=minHeight).padding(horizontal=16.dp,vertical=10.dp),verticalAlignment=Alignment.CenterVertically) {
        if(iconRes!=0 || icon!=null) { if(iconRes!=0)Icon(painterResource(iconRes),null,Modifier.size(22.dp),tint=MaterialTheme.colorScheme.onSurfaceVariant) else Icon(icon!!,null,Modifier.size(22.dp),tint=MaterialTheme.colorScheme.onSurfaceVariant);Spacer(Modifier.width(12.dp)) } else if(reserveIcon)Spacer(Modifier.width(34.dp))
        Column(Modifier.weight(1f),verticalArrangement=Arrangement.spacedBy(3.dp)) {
            Text(if(translateTitle)uiText(title)else title,fontSize=titleSize,lineHeight=20.sp,fontWeight=FontWeight.Medium)
            if(subtitle.isNotBlank()) Text(if(translateSubtitle)uiText(subtitle)else subtitle,fontSize=12.sp,lineHeight=16.sp,color=MaterialTheme.colorScheme.onSurfaceVariant,maxLines=summaryLines,overflow=TextOverflow.Ellipsis)
        }
        if(trailing!=null)trailing() else if(onClick!=null && chevron)Icon(Icons.Outlined.ChevronRight,null,Modifier.size(18.dp),tint=MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable internal fun LatencyAction(label:String,tag:String,onClick:()->Unit) {
    TextButton(onClick=onClick,modifier=Modifier.testTag(tag).defaultMinSize(minHeight=48.dp),contentPadding=PaddingValues(horizontal=8.dp)) {
        Text(uiText(label),fontSize=12.sp,maxLines=1)
    }
}

@Composable internal fun UiSection(title:String,trailing:String="",translateTitle:Boolean=true,action:(@Composable ()->Unit)?=null) {
    Row(Modifier.fillMaxWidth().padding(start=2.dp,top=10.dp,bottom=6.dp),verticalAlignment=Alignment.CenterVertically) {
        Text(if(translateTitle)uiText(title)else title,Modifier.weight(1f),fontSize=12.sp,lineHeight=16.sp,fontWeight=FontWeight.SemiBold,color=MaterialTheme.colorScheme.onSurface)
        if(trailing.isNotBlank())Text(trailing,fontSize=12.sp,lineHeight=16.sp,color=MaterialTheme.colorScheme.onSurfaceVariant)
        action?.invoke()
    }
}

@Composable internal fun UiMenu(actions:List<Pair<String,()->Unit>>,tag:String="more_menu",circle:Boolean=false) {
    var open by remember { mutableStateOf(false) }
    Box {
        if(circle) CompactActionButton(Icons.Outlined.MoreVert,"更多操作",tag,{open=true},iconSize=20.dp)
        else IconButton(onClick={open=true},modifier=Modifier.testTag(tag)) {Icon(Icons.Outlined.MoreVert,"更多操作")}
        DropdownMenu(expanded=open,onDismissRequest={open=false}) {
            actions.forEach { (title,action)-> DropdownMenuItem(text={Text(uiText(title))},onClick={open=false;action()}) }
        }
    }
}

@Composable internal fun UiPage(title:String,onDismiss:()->Unit,action:(@Composable RowScope.()->Unit)?=null,translateTitle:Boolean=true,
    content:@Composable (PaddingValues)->Unit) {
    val density=androidx.compose.ui.platform.LocalDensity.current
    val navigationInsets=WindowInsets.navigationBars
    val snackbar=LocalUiSnackbar.current
    val busy=LocalUiBusy.current
    val reload=LocalUiReload.current
    Dialog(onDismissRequest=onDismiss,properties=DialogProperties(usePlatformDefaultWidth=false,decorFitsSystemWindows=false)) {
        CompositionLocalProvider(androidx.compose.ui.platform.LocalDensity provides density) {
        val view=androidx.compose.ui.platform.LocalView.current
        val dark=MaterialTheme.colorScheme.onSurface.luminance()>.5f
        SideEffect { ((view.parent as? androidx.compose.ui.window.DialogWindowProvider) ?: (view as? androidx.compose.ui.window.DialogWindowProvider))?.window?.let{window->
            androidx.core.view.WindowCompat.setDecorFitsSystemWindows(window,false)
            // Compose handles IME insets here; Android's default pan would move the whole page a second time.
            if(android.os.Build.VERSION.SDK_INT>=31)window.setSoftInputMode(android.view.WindowManager.LayoutParams.SOFT_INPUT_ADJUST_NOTHING)
            window.addFlags(android.view.WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN or android.view.WindowManager.LayoutParams.FLAG_DRAWS_SYSTEM_BAR_BACKGROUNDS)
            window.setDimAmount(0f)
            if(android.os.Build.VERSION.SDK_INT>=28)window.attributes=window.attributes.apply {
                layoutInDisplayCutoutMode=if(android.os.Build.VERSION.SDK_INT>=30)android.view.WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_ALWAYS else android.view.WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES
                if(android.os.Build.VERSION.SDK_INT>=30)setFitInsetsTypes(0)
            }
            configureSystemBars(window,view,dark)
        } }
        Box(Modifier.fillMaxSize()) {
        UiBackdrop(Modifier.fillMaxSize(),home=false)
        Column(Modifier.fillMaxSize().semantics{testTagsAsResourceId=true}.windowInsetsPadding(WindowInsets.safeDrawing.union(navigationInsets))) {
        Scaffold(contentColor=MaterialTheme.colorScheme.onSurface,modifier=Modifier.weight(1f),contentWindowInsets=WindowInsets(0,0,0,0),containerColor=androidx.compose.ui.graphics.Color.Transparent,
            topBar={TopAppBar(windowInsets=WindowInsets(0,0,0,0),expandedHeight=56.dp,title={Text(if(translateTitle)uiText(title)else title,fontFamily=MaterialTheme.typography.titleLarge.fontFamily,fontSize=20.sp,fontWeight=FontWeight.SemiBold,maxLines=1,overflow=TextOverflow.Ellipsis)},navigationIcon={
                IconButton(onClick=onDismiss,modifier=Modifier.testTag("page_back")){Icon(Icons.AutoMirrored.Outlined.ArrowBack,"返回")}
            },actions={action?.invoke(this)},colors=TopAppBarDefaults.topAppBarColors(containerColor=androidx.compose.ui.graphics.Color.Transparent))},
            content=content)
        snackbar?.let { SnackbarHost(it,Modifier.fillMaxWidth().testTag("page_feedback_$title")) }
        reload?.let{ApplyChangesRow(it)}
        }
        if(busy)LinearProgressIndicator(Modifier.fillMaxWidth().systemBarsPadding().align(Alignment.TopCenter).testTag("page_busy"))
        }
        }
    }
}

@Composable internal fun UiPageList(title:String,onDismiss:()->Unit,action:(@Composable RowScope.()->Unit)?=null,
    state:androidx.compose.foundation.lazy.LazyListState?=null,translateTitle:Boolean=true,itemSpacing:Dp=10.dp,
    content:LazyListScope.()->Unit) {
    UiPage(title,onDismiss,action,translateTitle=translateTitle) {padding->
        LazyColumn(Modifier.fillMaxSize().padding(padding).consumeWindowInsets(padding).testTag("subpage_list"),state=state ?: LocalUiListState.current ?: androidx.compose.foundation.lazy.rememberLazyListState(),
            contentPadding=PaddingValues(start=14.dp,end=14.dp,top=12.dp,bottom=24.dp),verticalArrangement=Arrangement.spacedBy(itemSpacing),content=content)
    }
}

/** Dialog windows provide their own density; keep the application's chosen font scale in every slot. */
@Composable internal fun UiAlertDialog(onDismissRequest:()->Unit,confirmButton:@Composable ()->Unit,modifier:Modifier=Modifier,
    dismissButton:(@Composable ()->Unit)?=null,title:(@Composable ()->Unit)?=null,text:(@Composable ()->Unit)?=null,properties:DialogProperties=DialogProperties()) {
    val density=androidx.compose.ui.platform.LocalDensity.current
    AlertDialog(onDismissRequest=onDismissRequest,modifier=modifier,properties=properties,
        title=title?.let{slot->{CompositionLocalProvider(androidx.compose.ui.platform.LocalDensity provides density){slot()}}},
        text=text?.let{slot->{CompositionLocalProvider(androidx.compose.ui.platform.LocalDensity provides density){slot()}}},
        dismissButton=dismissButton?.let{slot->{CompositionLocalProvider(androidx.compose.ui.platform.LocalDensity provides density){slot()}}},
        confirmButton={CompositionLocalProvider(androidx.compose.ui.platform.LocalDensity provides density){confirmButton()}})
}

@Composable internal fun ChoiceDialog(title:String,value:String,choices:List<Pair<String,String>>,onDismiss:()->Unit,dismissOnChoose:Boolean=true,translateTitle:Boolean=true,translateChoices:Boolean=true,onChoose:(String)->Unit) {
    UiAlertDialog(onDismissRequest=onDismiss,modifier=Modifier.semantics{testTagsAsResourceId=true},title={Text(if(translateTitle)uiText(title)else title)},text={
        LazyColumn(Modifier.heightIn(max=440.dp).selectableGroup()) { items(choices.size) {i-> val (key,label)=choices[i]
            Row(Modifier.fillMaxWidth().testTag("choice_$key").selectable(selected=value==key,role=Role.RadioButton) {onChoose(key);if(dismissOnChoose)onDismiss()}.heightIn(min=48.dp),verticalAlignment=Alignment.CenterVertically) {
                RadioButton(selected=value==key,onClick=null);Spacer(Modifier.width(8.dp));Text(if(translateChoices)uiText(label)else label)
            }
        } }
    },confirmButton={TextButton(onClick=onDismiss){Text(uiText("取消"))}})
}

internal fun bytes(value:Long):String = when {
    value>=1073741824 -> "%.1f GB".format(value/1073741824.0)
    value>=1048576 -> "%.1f MB".format(value/1048576.0)
    value>=1024 -> "%.1f KB".format(value/1024.0)
    else -> "$value B"
}

@Suppress("DEPRECATION")
internal fun configureSystemBars(window:android.view.Window,view:android.view.View,dark:Boolean) {
    window.statusBarColor=android.graphics.Color.TRANSPARENT;window.navigationBarColor=android.graphics.Color.TRANSPARENT
    if(android.os.Build.VERSION.SDK_INT>=29) { window.isStatusBarContrastEnforced=false;window.isNavigationBarContrastEnforced=false }
    androidx.core.view.WindowCompat.getInsetsController(window,view).apply { isAppearanceLightStatusBars=!dark;isAppearanceLightNavigationBars=!dark }
}
