use std::{ffi::CStr, result, sync::{Arc, Mutex}, time::Duration};

use eldenring::{
    cs::{CSTaskGroupIndex, CSTaskImp, ChrIns, WorldChrMan},
    fd4::FD4TaskData,
};
use eldenring_util::{
    program::Program, singleton::get_instance,
    system::wait_for_system_init, task::CSTaskImpExt,
};
use std::mem::transmute;
use pelite::pe64::Pe;
use retour::static_detour;

#[derive(PartialEq)]
#[repr(C)]
pub enum LuaType
{
    LUATNONE = -1,
    LUATNIL = 0,
    LUATBOOLEAN = 1,
    LUATLIGHTUSERDATA = 2,
    LUATNUMBER = 3,
    LUATSTRING = 4,
    LUATTABLE = 5,
    LUATFUNCTION = 6,
    LUATUSERDATA = 7,
    LUATTHREAD = 8
}

pub struct Arg
{
    pub arg_type: String,
    pub arg_value: String,
}

pub struct Task
{
    pub act_id: i32,
    pub args: Vec<Arg>,
}

const HASARG_RVA: u32 = 0x14e58c0;
const GETARGTYPE_RVA: u32 = 0x14e9be0;
const GETNUMBER_RVA: u32 = 0x14ee8b0;
const GETINT_RVA: u32 = 0x14ee800;
const GETGLOBAL_RVA: u32 = 0x14e4570;
const GETSTRING_RVA: u32 = 0x14e26c0;
const CALL_RVA: u32 = 0x14e36f0;

pub struct GameFunctions
{
    pub hasarg_func: extern "C" fn(usize, i32) -> bool,
    pub getargtype_func: extern "C" fn(usize, i32) -> LuaType,
    pub getnumber_func: extern "C" fn(usize, i32) -> i32,
    pub getint_func: extern "C" fn(usize, i32) -> i32,
    pub getglobal_func: extern "C" fn(usize, usize),
    pub getstring_func: extern "C" fn(usize, i32) -> usize,
    pub call_func: extern "C" fn(usize, i32, i32),
}

impl GameFunctions
{
    fn default() -> GameFunctions{
        GameFunctions {
            hasarg_func: unsafe{transmute::<u64, extern "C" fn(usize, i32) -> bool>(Program::current().rva_to_va(HASARG_RVA).unwrap())},
            getargtype_func: unsafe {transmute::<u64, extern "C" fn(usize, i32) -> LuaType>(Program::current().rva_to_va(GETARGTYPE_RVA).unwrap())},
            getnumber_func: unsafe { transmute::<u64, extern "C" fn(usize, i32) -> i32>(Program::current().rva_to_va(GETNUMBER_RVA).unwrap())},
            getint_func: unsafe{transmute::<u64, extern "C" fn(usize, i32) -> i32>(Program::current().rva_to_va(GETINT_RVA).unwrap())},
            getglobal_func: unsafe{transmute::<u64, extern "C" fn(usize, usize)>(Program::current().rva_to_va(GETGLOBAL_RVA).unwrap())},
            getstring_func: unsafe{transmute::<u64, extern "C" fn(usize, i32) -> usize>(Program::current().rva_to_va(GETSTRING_RVA).unwrap())},
            call_func: unsafe{transmute::<u64, extern "C" fn(usize, i32, i32)>(Program::current().rva_to_va(CALL_RVA).unwrap())}
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn DllMain(_hmodule: u64, reason: u32) -> bool {
    if reason != 1 {
        return true;
    }

    let task_list: Arc<Mutex<Vec<Task>>> = Arc::new(Mutex::new(Vec::new()));
    let task_list_clone = Arc::clone(&task_list);

    let hks_act_va = Program::current().rva_to_va(0x40ccd0).unwrap();

    static_detour! {
        static HKSACT_HOOK: unsafe extern "C" fn(*const *mut ChrIns, i32, usize) -> i32;
    }

    unsafe{
    HKSACT_HOOK.initialize(std::mem::transmute::<u64, unsafe extern "C" fn(*const *mut ChrIns, i32, usize) -> i32>(hks_act_va), 
     move |chr_ins_holder: *const *mut ChrIns, act_id, hks_state|
    {
        if act_id == 1000
        {
            let chr_ins = &&**chr_ins_holder;
            let Some(world_chr_man) = get_instance::<WorldChrMan>().unwrap() else {
                return 0;
            };
            let Some(ref mut main_player ) = world_chr_man.main_player else {
                return 0;
            };

            if chr_ins.field_ins_handle != main_player.chr_ins.field_ins_handle
            {
                return 0;
            }

            let game_functions = GameFunctions::default();
            if (game_functions.getargtype_func)(hks_state, 2) == LuaType::LUATNUMBER && (game_functions.getargtype_func)(hks_state, 3) == LuaType::LUATSTRING
            {
                let function_name = (game_functions.getstring_func)(hks_state, 3);
                (game_functions.getglobal_func)(hks_state, function_name);
                (game_functions.call_func)(hks_state, 0, 0);
            }
        }
        else if act_id == 1001
        {
            let game_functions = GameFunctions::default();
            let mut task_list_lock = task_list_clone.lock().unwrap();
            let string_to_print = (game_functions.getstring_func)(hks_state, 2) as *const i8;
            let string_to_print = CStr::from_ptr(string_to_print).to_str().unwrap().to_string();
            task_list_lock.push(Task { act_id: 1001, args: vec![Arg{arg_type: "String".to_string(), arg_value: string_to_print}] });
        }

        return HKSACT_HOOK.call(chr_ins_holder, act_id, hks_state);
    }).unwrap().enable().unwrap();
    }

    std::thread::spawn(move|| {
        wait_for_system_init(&Program::current(), Duration::MAX)
            .expect("Timeout waiting for system init");

        let cs_task = unsafe{get_instance::<CSTaskImp>()}.unwrap().unwrap();
        cs_task.run_recurring(
            move   |_: &FD4TaskData| {
                let mut task_list_lock = task_list.lock().unwrap();

                let mut done_tasks: Vec<usize> = Vec::new();

                for (i, task) in task_list_lock.iter().enumerate()
                {
                    if task.act_id == 1001
                    {
                        println!("{}", task.args[0].arg_value); 
                        done_tasks.push(i);
                    }
                }

                for i in done_tasks.iter().rev()
                {
                    task_list_lock.remove(*i);
                }
            },
            CSTaskGroupIndex::FrameBegin,
        );
        }
    );

    true
}