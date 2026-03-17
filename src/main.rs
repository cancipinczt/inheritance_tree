use regex::Regex;
use std::fs;
use syn::{GenericArgument, Item, ItemMod, ItemUse, PathArguments, Type, UseTree};


#[derive(Debug, Clone, PartialEq)]
pub enum ClassType {
    Normal,     // 普通类
    Mixin,      // Mixin类
    Composite,  // Mixin Instance
}

#[derive(Debug, Clone)]
pub struct ClassInfo {
    pub name: String,
    pub class_type: ClassType,
    pub extends: Option<String>,
    pub implements: Vec<String>,
    pub with: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct MixinInfo {
    pub name: String,
    pub host_classes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ModuleInfo {
    pub name: String,
    pub classes: Vec<ClassInfo>,
    pub mixins: Vec<MixinInfo>,
}

fn extract_class_type_from_vtable(item_mod: &ItemMod) -> (ClassType, Option<String>) {
    // 递归查找vtable模块
    if let Some((_, items)) = &item_mod.content {
        // 首先在当前模块的直接子模块中查找vtable模块
        for item in items {
            if let Item::Mod(vtable_mod) = item {
                let vtable_mod_name = vtable_mod.ident.to_string();
                
                // 检查是否是vtable模块（可能以vtable开头或包含vtable）
                if vtable_mod_name.contains("vtable") || vtable_mod_name == "vtable" {
                    println!("DEBUG: Found vtable module: {}", vtable_mod_name);
                    
                    // 在vtable模块中查找TYPE静态变量定义
                    if let Some((_, vtable_items)) = &vtable_mod.content {
                        for vitem in vtable_items {
                            if let Item::Static(static_item) = vitem {
                                if static_item.ident.to_string() == "TYPE" {
                                    let type_expr = quote::quote!(#static_item).to_string();
                                    println!("DEBUG: Found TYPE static variable: {}", type_expr);
                                    
                                    // 检查TYPE静态变量的类型
                                    if type_expr.contains("TypeInfo :: new_mixin_instance") || type_expr.contains("TypeInfo::new_mixin_instance") ||
                                       type_expr.contains("new_mixin_instance") || type_expr.contains("mixin_instance") {
                                        println!("DEBUG: Identified as Composite class");
                                        return (ClassType::Composite, None);
                                    }
                                    
                                    if type_expr.contains("TypeInfo :: new_mixin") || type_expr.contains("TypeInfo::new_mixin") ||
                                       type_expr.contains("new_mixin") {
                                        println!("DEBUG: Identified as Mixin class");
                                        return (ClassType::Mixin, None);
                                    }
                                    
                                    if type_expr.contains("TypeInfo :: new_concrete_class") || type_expr.contains("TypeInfo::new_concrete_class") || 
                                       type_expr.contains("TypeInfo :: new_abstract_class") || type_expr.contains("TypeInfo::new_abstract_class") ||
                                       type_expr.contains("new_concrete_class") || type_expr.contains("new_abstract_class") {
                                        println!("DEBUG: Identified as Normal class");
                                        return (ClassType::Normal, None);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        
        // 如果没有找到vtable模块，递归查找所有子模块
        for item in items {
            if let Item::Mod(sub_mod) = item {
                // 递归查找子模块中的vtable模块
                let (class_type, _) = extract_class_type_from_vtable(sub_mod);
                // 如果找到非Normal类型，立即返回
                if class_type != ClassType::Normal {
                    return (class_type, None);
                }
            }
        }
    }
    
    // 如果无法确定类型，默认为普通类
    (ClassType::Normal, None)
}

fn extract_super_class(item_mod: &ItemMod) -> Option<String> {
    // 使用AST来提取Super类型定义
    if let Some((_, items)) = &item_mod.content {
        for item in items {
            if let Item::Type(item_type) = item {
                if item_type.ident.to_string() == "Super" {
                    let super_type_str = quote::quote!(#item_type).to_string();
                    
                    println!("DEBUG: Found Super type via AST: {}", super_type_str);
                    
                    // 提取类型表达式
                    if let Type::Path(type_path) = &*item_type.ty {
                        let type_str = quote::quote!(#type_path).to_string();
                        
                        // 如果是组合类模式，提取组合类名
                        if type_str.contains("MixinWith") {
                            println!("DEBUG: Detected composite class pattern");
                            if let Some(composite_name) = extract_composite_class_name(&type_str) {
                                println!("DEBUG: Extracted composite name: {}", composite_name);
                                return Some(composite_name);
                            } else {
                                println!("DEBUG: Failed to extract composite name");
                            }
                        }
                        
                        // 如果是::classes::object::Object类，直接返回Object
                        if type_str.contains("::") {
                            if let Some(last_part) = type_str.split("::").last() {
                                return Some(last_part.trim().to_string());
                            }
                        }

                        // 消除组合类父类类似A<T, V>的影响，只保留A
                        if type_str.contains("<") {
                            if let Some(first_part) = type_str.split("<").next() {
                                return Some(first_part.trim().to_string());
                            }
                        }
                        
                        return Some(type_str);
                    }
                }
            }
        }
    }
    
    None
}

fn extract_composite_class_name(super_type: &str) -> Option<String> {
    // 提取组合类名：从类似"<A<...> as MixinWith<M>>::Instance<T, V>"的模式中提取"A_M1_M2"
    
    println!("DEBUG: Processing super_type: {}", super_type);
    
    // 提取所有类名和Mixin名
    let mut parts = Vec::new();
    
    // 提取第一个类名（在第一个<之后，as之前）
    let first_class_pattern = Regex::new(r"<\s*([A-Za-z_][A-Za-z0-9_]*)").unwrap();
    if let Some(caps) = first_class_pattern.captures(super_type) {
        let first_class = caps.get(1).unwrap().as_str();
        println!("DEBUG: Extracted first class: {}", first_class);
        parts.push(first_class.to_string());
    }
    
    // 提取所有MixinWith中的Mixin名
    println!("DEBUG: Looking for MixinWith patterns...");
    println!("DEBUG: Super type string length: {}", super_type.len());
    
    // 首先检查字符串中是否包含MixinWith
    if super_type.contains("MixinWith") {
        println!("DEBUG: String contains 'MixinWith'");
        
        // 找到所有MixinWith的位置
        let mut mixin_positions = Vec::new();
        for (i, _) in super_type.match_indices("MixinWith") {
            mixin_positions.push(i);
        }
        println!("DEBUG: Found 'MixinWith' at positions: {:?}", mixin_positions);
        
        // 使用更精确的字符串搜索方法
        let mut current_pos = 0;
        let mut mixin_count = 0;
        
        // 先检查字符串中是否包含"MixinWith<"
        println!("DEBUG: Checking if string contains 'MixinWith<': {}", super_type.contains("MixinWith<"));
        
        // 尝试搜索"MixinWith<"
        while let Some(pos) = super_type[current_pos..].find("MixinWith<") {
            let mixin_start = current_pos + pos + "MixinWith<".len();
            println!("DEBUG: Found 'MixinWith<' at position: {}", current_pos + pos);
            println!("DEBUG: Mixin name starts at position: {}", mixin_start);
            
            // 查找Mixin名结束的位置
            if let Some(mixin_end) = super_type[mixin_start..].find(|c: char| c == '>' || c == ' ' || c == ',') {
                let mixin_name = &super_type[mixin_start..mixin_start + mixin_end];
                let trimmed_name = mixin_name.trim();
                println!("DEBUG: Raw mixin name: '{}'", mixin_name);
                println!("DEBUG: Trimmed mixin name: '{}'", trimmed_name);
                
                if !trimmed_name.is_empty() {
                    println!("DEBUG: Extracted mixin name: '{}'", trimmed_name);
                    parts.push(trimmed_name.to_string());
                    mixin_count += 1;
                }
                current_pos = mixin_start + mixin_end;
            } else {
                println!("DEBUG: No end marker found for mixin name");
                break;
            }
        }
        
        // 如果没找到"MixinWith<", 尝试搜索"MixinWith <"（带空格）
        if mixin_count == 0 {
            println!("DEBUG: Trying 'MixinWith <' with space...");
            current_pos = 0;
            while let Some(pos) = super_type[current_pos..].find("MixinWith <") {
                let mixin_start = current_pos + pos + "MixinWith <".len();
                println!("DEBUG: Found 'MixinWith <' at position: {}", current_pos + pos);
                println!("DEBUG: Mixin name starts at position: {}", mixin_start);
                
                // 查看从mixin_start开始的字符串内容
                let remaining_str = &super_type[mixin_start..];
                println!("DEBUG: String from mixin_start: '{}'", remaining_str);
                
                // 跳过前面的空格
                let mixin_name_start = remaining_str.find(|c: char| c.is_alphanumeric() || c == '_').unwrap_or(0);
                let mixin_name_str = &remaining_str[mixin_name_start..];
                println!("DEBUG: String after skipping spaces: '{}'", mixin_name_str);
                
                // 查找Mixin名结束的位置 - 找第一个非字母数字下划线字符
                if let Some(mixin_end) = mixin_name_str.find(|c: char| !c.is_alphanumeric() && c != '_') {
                    let mixin_name = &mixin_name_str[..mixin_end];
                    let trimmed_name = mixin_name.trim();
                    println!("DEBUG: Raw mixin name: '{}'", mixin_name);
                    println!("DEBUG: Trimmed mixin name: '{}'", trimmed_name);
                    
                    if !trimmed_name.is_empty() {
                        println!("DEBUG: Extracted mixin name: '{}'", trimmed_name);
                        parts.push(trimmed_name.to_string());
                        mixin_count += 1;
                    }
                    current_pos = mixin_start + mixin_name_start + mixin_end;
                } else {
                    println!("DEBUG: No end marker found for mixin name");
                    break;
                }
            }
        }
        
        println!("DEBUG: Total mixins found: {}", mixin_count);
    } else {
        println!("DEBUG: String does NOT contain 'MixinWith'");
    }
    
    println!("DEBUG: Found {} mixins", parts.len() - 1); // 减去第一个类名
    
    // 如果parts为空，说明没有找到有效的类名
    if parts.is_empty() {
        println!("DEBUG: No parts found");
        return None;
    }
    
    println!("DEBUG: All parts: {:?}", parts);
    
    // 组合成A_M1_M2_M3...的形式
    let composite_name = parts.join("_");
    
    println!("DEBUG: Final composite name: {}", composite_name);
    Some(composite_name)
}

fn extract_mixin_host_classes(parent_mod: &ItemMod, mixin_name: &str) -> Vec<String> {
    let mut host_classes = Vec::new();
    
    println!("DEBUG: Processing mixin '{}'", mixin_name);
    
    // 直接从AST中提取use语句信息
    if let Some((_, items)) = &parent_mod.content {
        for item in items {
            if let Item::Use(use_item) = item {
                // 处理use语句
                extract_use_info(use_item, mixin_name, &mut host_classes);
            }
        }
    }
    
    println!("DEBUG: Final host classes for {}: {:?}", mixin_name, host_classes);
    host_classes
}

fn extract_use_info(use_item: &ItemUse, mixin_name: &str, host_classes: &mut Vec<String>) {
    // 遍历use语句的路径
    fn traverse_use_tree(tree: &UseTree, mixin_name: &str, host_classes: &mut Vec<String>) {
        match tree {
            UseTree::Path(use_path) => {
                // 检查路径是否指向_classes
                let seg = use_path.ident.to_string();
                if seg == "_classes" {
                    // 处理_classes的use语句
                    traverse_use_tree(&use_path.tree, mixin_name, host_classes);
                }
            }
            UseTree::Name(use_name) => {
                // 单个类名：use _classes::A_M1;
                let class_name = use_name.ident.to_string();
                println!("DEBUG: Found single use: {}", class_name);
                if let Some(host_class) = class_name.strip_suffix(&format!("_{}", mixin_name)) {
                    println!("DEBUG: Extracted host class: {} from {}", host_class, class_name);
                    host_classes.push(host_class.to_string());
                }
            }
            UseTree::Group(use_group) => {
                // 多个类名：use _classes::{A_M1, A_M2};
                for item in &use_group.items {
                    if let UseTree::Name(use_name) = item {
                        let class_name = use_name.ident.to_string();
                        println!("DEBUG: Found multi use: {}", class_name);
                        if let Some(host_class) = class_name.strip_suffix(&format!("_{}", mixin_name)) {
                            println!("DEBUG: Extracted host class: {} from {}", host_class, class_name);
                            host_classes.push(host_class.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    
    traverse_use_tree(&use_item.tree, mixin_name, host_classes);
}

fn extract_inheritance_info_by_module(file_content: &str) -> Vec<ModuleInfo> {
    let syntax = syn::parse_file(file_content).expect("Failed to parse file");
    let mut modules = Vec::new();
    
    fn process_module(items: &[Item], current_path: &str, modules: &mut Vec<ModuleInfo>) {
        for item in items {
            if let Item::Mod(item_mod) = item {
                let mod_name = item_mod.ident.to_string();
                let new_path = if current_path.is_empty() {
                    mod_name.clone()
                } else {
                    format!("{}::{}", current_path, mod_name)
                };
                
                // 检查是否包含_classes模块（最小一级模块）
                if let Some((_, items)) = &item_mod.content {
                    let has_classes = items.iter().any(|item| {
                        if let Item::Mod(sub_mod) = item {
                            sub_mod.ident.to_string() == "_classes"
                        } else {
                            false
                        }
                    });
                    
                    if has_classes {
                        // 这是最小一级模块，提取继承信息
                        // 需要深入到_classes模块中提取类信息
                        if let Some(class_items) = find_classes_module(items) {
                            let module_info = extract_classes_from_module(class_items, &new_path, item_mod);
                            modules.push(module_info);
                        }
                    }

                    // 继续递归处理子模块
                    if let Some((_, sub_items)) = &item_mod.content {
                        process_module(sub_items, &new_path, modules);
                    }
                }
            }
        }
    }
    
    process_module(&syntax.items, "", &mut modules);
    modules
}

fn extract_classes_from_module(class_items: &[Item], new_path: &str, parent_mod: &ItemMod) -> ModuleInfo {
    let mut classes = Vec::new();
    let mut mixins = Vec::new();
    
    // class_items 已经是 _classes 模块的内容，直接处理其中的类模块
    for item in class_items {
        if let Item::Mod(class_mod) = item {
            let class_mod_name = &class_mod.ident.to_string();
            
            // 只处理以下划线开头的模块（类定义）
            if class_mod_name.starts_with('_') {
                let class_name = class_mod_name.trim_start_matches('_').to_string();
                
                let (class_type, _) = extract_class_type_from_vtable(class_mod);
                
                if matches!(class_type, ClassType::Mixin) {
                    // 添加Mixin类
                    let host_classes = extract_mixin_host_classes(parent_mod, class_name.as_str());
                    let mixin_info = MixinInfo {
                        name: class_name.clone(),
                        host_classes,
                    };
                    mixins.push(mixin_info);

                    // 添加组合类
                    let composite_classes = extract_composite_classes_from_mixin_module(class_mod, class_name.as_str());
                    classes.extend(composite_classes);
                } else {
                    // 添加普通类
                    let extends = extract_super_class(class_mod);
                    let implements = extract_implements(class_mod);
                    
                    let with = if let Some(ref parent) = extends {
                        // 从组合类名中提取Mixin列表
                        if parent.contains('_') {
                            parent.split('_').skip(1).map(|s| s.to_string()).collect()
                        } else {
                            Vec::new()
                        }
                    } else {
                        Vec::new()
                    };
                    
                    let class_info = ClassInfo {
                        name: class_name,
                        class_type,
                        extends,
                        implements,
                        with,
                    };
                    classes.push(class_info);
                }
            }
        }
    }
    
    ModuleInfo {
        name: new_path.to_string(),
        classes,
        mixins,
    }
}

fn find_classes_module(items: &[Item]) -> Option<&[Item]> {
    for item in items {
        if let Item::Mod(item_mod) = item {
            if item_mod.ident.to_string() == "_classes" {
                if let Some((_, sub_items)) = &item_mod.content {
                    return Some(sub_items);
                }
            }
        }
    }
    None
}

fn extract_composite_classes_from_mixin_module(mixin_mod: &ItemMod, mixin_name: &str) -> Vec<ClassInfo> {
    let mut composite_classes = Vec::new();
    
    if let Some((_, items)) = &mixin_mod.content {
        for item in items {
            if let Item::Mod(sub_mod) = item {
                let sub_mod_name = sub_mod.ident.to_string();
                
                // 检查是否是组合类模块（以下划线开头，且名称格式为 _A_M1 或类似）
                if sub_mod_name.starts_with('_') && sub_mod_name.len() > 1 {
                    // 检查模块名称是否包含Mixin名
                    if sub_mod_name.contains(mixin_name) {
                        println!("=== CHECKING COMPOSITE MODULE: {} (contains mixin {}) ===", sub_mod_name, mixin_name);
                    } else {
                        continue;
                    }
                    
                    let class_name = sub_mod_name.trim_start_matches('_').to_string();
                    let mut is_composite_class = false;
                    
                    // 在该模块中寻找vtable模块
                    if let Some((_, sub_items)) = &sub_mod.content {
                        'vtable_search: for sub_item in sub_items {
                            if let Item::Mod(vtable_mod) = sub_item {
                                let vtable_mod_name = vtable_mod.ident.to_string();
                                
                                // 检查是否是vtable模块
                                if vtable_mod_name.contains("vtable") || vtable_mod_name == "vtable" {
                                    // 检查vtable模块中的TYPE静态变量
                                    if let Some((_, vtable_items)) = &vtable_mod.content {
                                        for vitem in vtable_items {
                                            if let Item::Static(static_item) = vitem {
                                                if static_item.ident.to_string() == "TYPE" {
                                                    let type_expr = quote::quote!(#static_item).to_string();
                                                    
                                                    // 检查TYPE静态变量的类型
                                                    if type_expr.contains("TypeInfo :: new_mixin_instance") || type_expr.contains("TypeInfo::new_mixin_instance") {
                                                        println!("=== COMPOSITE CLASS FOUND: {} ===", class_name);
                                                        is_composite_class = true;
                                                        break 'vtable_search;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    
                    if is_composite_class {
                        let extends = extract_super_class(sub_mod);
                        let implements = extract_implements(sub_mod);
                        
                        // 对于组合类，with字段应该包含该组合类所使用的Mixin列表
                        // 组合类名格式为 A_M1_M2，其中A是基类，M1、M2是Mixin
                        // let with = if class_name.contains('_') {
                        //     class_name.split('_').skip(1).map(|s| s.to_string()).collect()
                        // } else {
                        //     Vec::new()
                        // };
                        let with = Vec::new();
                        
                        let class_info = ClassInfo {
                            name: class_name,
                            class_type: ClassType::Composite,
                            extends,
                            implements,
                            with,
                        };
                        
                        composite_classes.push(class_info);
                    }
                }
            }
        }
    }
    
    composite_classes
}

fn extract_implements(item_mod: &ItemMod) -> Vec<String> {
    let mut interfaces = Vec::new();
    
    // 在类模块的直接内容中查找接口实现
    if let Some((_, items)) = &item_mod.content {
        for item in items {
            if let Item::Impl(item_impl) = item {
                // 检查是否是trait实现
                if let Some((_, trait_path, _)) = &item_impl.trait_ {
                    // 检查是否是HasImpl trait
                    if is_has_impl_trait(trait_path) {
                        // 提取接口名
                        if let Some(interface) = extract_interface_from_has_impl(trait_path) {
                            // 避免重复添加
                            if !interfaces.contains(&interface) {
                                interfaces.push(interface);
                            }
                        }
                    }
                }
            }
        }
    }
    
    interfaces
}

// 检查是否是 HasImpl trait
fn is_has_impl_trait(path: &syn::Path) -> bool {
    path.segments.last()
        .map(|seg| seg.ident == "HasImpl")
        .unwrap_or(false)
}

// 从 HasImpl<Interface> 中提取接口名
fn extract_interface_from_has_impl(path: &syn::Path) -> Option<String> {
    if let Some(last_seg) = path.segments.last() {
        if let PathArguments::AngleBracketed(args) = &last_seg.arguments {
            if let Some(GenericArgument::Type(ty)) = args.args.first() {
                return Some(extract_type_name_from_type(ty));
            }
        }
    }
    None
}

// 从 Type 中提取类型名
fn extract_type_name_from_type(ty: &Type) -> String {
    match ty {
        Type::Path(type_path) => extract_type_name(type_path),
        _ => "Unknown".to_string(),
    }
}

// 从 TypePath 中提取类型名
fn extract_type_name(type_path: &syn::TypePath) -> String {
    if let Some(last_seg) = type_path.path.segments.last() {
        last_seg.ident.to_string()
    } else {
        "Unknown".to_string()
    }
}

fn format_inheritance_tree(modules: &[ModuleInfo]) -> String {
    let mut output = String::new();
    
    for module in modules {
        output.push_str(&format!("=== Module: {} ===\n\n", module.name));
        
        // 输出Mixin类信息
        if !module.mixins.is_empty() {
            for mixin in &module.mixins {
                output.push_str(&format!("Mixin: {}\n", mixin.name));
                output.push_str(&format!("  HostClasses: [{}]\n", mixin.host_classes.join(", ")));
                output.push_str("\n");
            }
        }
        
        // 输出组合类（Mixin Instance）信息
        let composite_classes: Vec<_> = module.classes.iter()
            .filter(|c| matches!(c.class_type, ClassType::Composite))
            .collect();
        
        if !composite_classes.is_empty() {
            for class in composite_classes {
                output.push_str(&format!("Mixin Instance: {}\n", class.name));
                if let Some(ref parent) = class.extends {
                    output.push_str(&format!("  Extends: {}\n", parent));
                }
                output.push_str(&format!("  Implements: [{}]\n", class.implements.join(", ")));
                output.push_str("\n");
            }
        }
        
        // 输出普通类信息
        let normal_classes: Vec<_> = module.classes.iter()
            .filter(|c| !matches!(c.class_type, ClassType::Composite))
            .collect();
        
        if !normal_classes.is_empty() {
            for class in normal_classes {
                output.push_str(&format!("Class: {}\n", class.name));
                if let Some(ref parent) = class.extends {
                    output.push_str(&format!("  Extends: {}\n", parent));
                }
                output.push_str(&format!("  Implements: [{}]\n", class.implements.join(", ")));
                output.push_str(&format!("  With: [{}]\n", class.with.join(", ")));
                output.push_str("\n");
            }
        }
        
        output.push_str("\n");
    }
    
    output
}

fn main() {
    let file_path = "./test/run_e.rs";
    let output_path = "inheritance_tree.txt";
    
    println!("Reading file: {}", file_path);
    let content = fs::read_to_string(file_path).expect("Failed to read file");
    
    println!("Extracting inheritance information...");
    let modules = extract_inheritance_info_by_module(&content);
    
    println!("Formatting output...");
    let output = format_inheritance_tree(&modules);
    
    println!("Writing to file: {}", output_path);
    fs::write(output_path, output).expect("Failed to write output file");
    
    println!("Analysis completed!");
    
    // 输出统计信息
    let total_modules = modules.len();
    let total_classes: usize = modules.iter()
        .map(|m| m.classes.iter()
            .filter(|c| !matches!(c.class_type, ClassType::Mixin))
            .count())
        .sum();
    let total_mixins: usize = modules.iter().map(|m| m.mixins.len()).sum();
    
    println!("\nStatistics:");
    println!("Total modules analyzed: {}", total_modules);
    println!("Total classes found: {}", total_classes);
    println!("Total mixins found: {}", total_mixins);
}