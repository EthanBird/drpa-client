from __future__ import annotations

import time
from dataclasses import dataclass
from datetime import datetime, timedelta

from DrissionPage import ChromiumPage, ChromiumOptions


@dataclass(frozen=True)
class Config:
    system_url: str
    business_type: str
    loop_count: int
    work_date_offset_days: int
    responsible_person: str
    assistant_person: str
    operator_person: str
    responsible_phone: str
    work_location_prefix: str
    plan_content_prefix: str
    plan_content_remove_text: str
    survey_mode: str
    specialty_category: str
    safety_job_type: str
    plan_start_time: tuple[str, str, str]
    plan_end_time: tuple[str, str, str]
    wait_before_each_loop_seconds: int
    headless: bool

    @property
    def work_date(self):
        return datetime.today() + timedelta(days=self.work_date_offset_days)

    @property
    def work_day(self) -> int:
        return self.work_date.day

    @property
    def is_weekend(self) -> bool:
        return self.work_date.isoweekday() in [6, 7]


def main(ctx):
    config = load_config(ctx.params)
    bot = MarketingCancelOrderDispatchBot(ctx, config)
    bot.run()


def load_config(params) -> Config:
    return Config(
        system_url=str(params.get("system_url")),
        business_type=str(params.get("business_type") or "销户"),
        loop_count=int(params.get("loop_count") or 10),
        work_date_offset_days=int(params.get("work_date_offset_days") or 1),
        responsible_person=str(params.get("responsible_person") or ""),
        assistant_person=str(params.get("assistant_person") or ""),
        operator_person=str(params.get("operator_person") or ""),
        responsible_phone=str(params.get("responsible_phone") or ""),
        work_location_prefix=str(params.get("work_location_prefix") or ""),
        plan_content_prefix=str(params.get("plan_content_prefix") or ""),
        plan_content_remove_text=str(params.get("plan_content_remove_text") or ""),
        survey_mode=str(params.get("survey_mode") or "否"),
        specialty_category=str(params.get("specialty_category") or "计量专业"),
        safety_job_type=str(params.get("safety_job_type") or "营销运维"),
        plan_start_time=parse_time(params.get("plan_start_time") or "08:00:00"),
        plan_end_time=parse_time(params.get("plan_end_time") or "19:59:59"),
        wait_before_each_loop_seconds=int(params.get("wait_before_each_loop_seconds") or 10),
        headless=bool(params.get("headless", False)),
    )


def parse_time(value: str) -> tuple[str, str, str]:
    parts = str(value).split(":")
    if len(parts) != 3:
        raise ValueError(f"时间格式必须为 HH:MM:SS：{value}")
    return parts[0], parts[1], parts[2]


class MarketingCancelOrderDispatchBot:
    def __init__(self, ctx, config: Config):
        self.ctx = ctx
        self.config = config
        self.page = None
        self.tab = None

    def run(self):
        self.ctx.log.info("启动营销销户派工自动化，系统：%s", self.config.system_url)
        self.open_system()
        for index in range(self.config.loop_count):
            self.ctx.progress(index / max(self.config.loop_count, 1) * 100, f"开始第 {index + 1} 轮")
            time.sleep(self.config.wait_before_each_loop_seconds)
            order_no = self.get_order_no(self.config.business_type)
            self.ctx.log.info("获取工单号：%s", order_no)
            if self.sign_order(order_no):
                self.on_site_power_off()
            if self.enter_personal_todo_order(order_no):
                self.confirm_popup()
                self.choose_person_transfer(self.config.operator_person)
                self.confirm_popup()
                self.assign_metering_device_task()
            self.tab.refresh()
        self.ctx.progress(100, "全部循环处理完成")

    def open_system(self):
        options = ChromiumOptions()
        options.headless(self.config.headless)
        self.page = ChromiumPage(options)
        self.page.get(self.config.system_url)
        self.page.ele("@text():营销管理系统").click()
        self.tab = self.page.latest_tab

    def get_order_no(self, keyword="销户"):
        time.sleep(10)
        self.tab.eles("岗位待办")[-1].click()
        self.tab.ele("@text()=高级查询").click()
        self.tab.ele("#ywflId").ele("业扩管理").click()
        self.tab.ele("#ywlb").ele("销户").click()
        self.tab.ele("#gzdbh").run_js("taskList.retriveAll()")
        return self.tab.ele(f"@@text()={keyword}@@class=hy_grid_cell").prev().child().attr("title")

    def open_order_mon(self):
        # TODO: 原始脚本未实现。需要根据业务系统页面补充“工作单监控”入口。
        raise NotImplementedError("open_order_mon() 尚未实现")

    def select_work_order(self, order_no):
        # TODO: 原始脚本未实现。需要补充查询并选择改派工单逻辑。
        raise NotImplementedError("select_work_order() 尚未实现")

    def select_user(self, who):
        # TODO: 原始脚本未实现。需要补充查询选择用户逻辑。
        raise NotImplementedError("select_user() 尚未实现")

    def save(self, message="改派原因"):
        # TODO: 原始脚本未实现。需要补充保存改派原因逻辑。
        raise NotImplementedError("save() 尚未实现")

    def change_order_to(self, order_no, who="孟繁宇"):
        self.open_order_mon()
        self.select_work_order(order_no)
        self.select_user(who)
        self.save()

    def sign_order(self, order_no):
        time.sleep(2)
        self.tab.ele(f"@title={order_no}").click(by_js=True)
        time.sleep(1)
        while len(self.tab.eles("@class=popWindowOKBtnStyle", timeout=5)) != 0:
            self.tab.ele("@class=popWindowOKBtnStyle").click()
            time.sleep(1)
        return True

    def on_site_power_off(self):
        self.tab.eles("#img_dateformatdate_0")[0].click()
        self.tab.eles("#selectButton")[0].click()
        self.tab.ele("保存").click()
        self.tab.wait.ele_displayed("保存成功")
        self.tab.wait.ele_deleted("保存成功")
        self.tab.ele("@@text()=返回@@align=center").click()
        return True

    def enter_personal_todo_order(self, order_no):
        self.tab.wait.ele_displayed("@@class=xiugaihoude1@@text()=个人待办")
        if self.tab.ele("@@class=xiugaihoude1@@text()=个人待办").click():
            return self.tab.ele(f"@title={order_no}").parent().prev().child().click()
        return False

    def choose_person_transfer(self, who):
        self.tab.ele("选择人员传递").click()
        self.tab.wait.ele_displayed("人员名称：")
        self.tab.ele("#rymc").input(who, clear=True)
        self.tab.ele("@text()=查询").click()
        self.tab.ele(f"@@key=5@@text()={who}").click.multi()
        self.tab.wait.ele_displayed("@text():正在处理。")
        self.tab.wait.ele_deleted("@text():正在处理。")
        return True

    def remove_meter_box_operation(self):
        self.tab.ele("计量配表").click()
        self.tab.ele("#tabId_jlxtabId_tbcT").click()
        self.tab.ele("#onRemoveJlxId").click()
        self.tab.ele("#ajaxgridDiv_Top").ele("#input_checkbox").click()
        self.tab.ele("#ccBtn").click()
        self.tab.wait.ele_displayed("拆除成功")
        self.tab.wait.ele_deleted("拆除成功")
        self.tab.ele("关闭").click()
        time.sleep(1)
        self.tab.ele("#ajaxgridjlxzc_gridDiv").ele("@tag()=tbody").child().child().click.multi()
        self.tab.wait.ele_displayed("执行班组：")
        self.tab.ele("#ajaxform_query_zxbzbmId_ffInput0").focus()
        self.tab.ele("#zxbzbmId_img").click()
        self.tab.wait.ele_displayed("计量运维及用电检查班")
        self.tab.ele("计量运维及用电检查班").click.multi()
        self.tab.ele("#onSaveJlxId").click()
        time.sleep(1)
        self.tab.ele("#tabId_fptabId_tbcT").click()

    def assign_metering_device_task(self):
        time.sleep(1)
        self.tab.ele("业务详细信息").click()
        self.tab.wait.ele_displayed("选择负责人：")
        if self.tab.wait.ele_displayed("该用户有运行表箱", timeout=5):
            self.tab.ele("@class=popWindowOKBtnStyle").click()
            self.remove_meter_box_operation()
        self.choose_person_assign_and_transfer()
        self.send_job_info()

    def choose_person_assign_and_transfer(self):
        self.tab.ele("#ajaxform1_fzrId_ffInput0").run_js("fzrId.imgClick();chooseFzr();")
        self.tab.wait.ele_displayed("人员名称：")
        self.tab.ele("#rymc").input(self.config.responsible_person, clear=True, by_js=True)
        self.tab.ele("#rymc").run_js("retrieve()")
        self.tab.ele("#ajaxgrid_gridDiv").ele(self.config.responsible_person).click.multi()

        self.tab.ele("#ajaxform1_kcry1Id_ffInput0").run_js("kcry1Id.imgClick();chooseZby1();")
        self.tab.wait.ele_displayed("人员名称：")
        self.tab.ele("#rymc").input(self.config.assistant_person, clear=True, by_js=True)
        self.tab.ele("#rymc").run_js("retrieve()")
        self.tab.ele("#ajaxgrid_gridDiv").ele(self.config.assistant_person).click.multi()

        self.tab.ele("@checkid=ajaxgrid_top_1_0_checkFlag").click()
        self.tab.ele("#pg").run_js("onPG()")
        try:
            self.tab.ele("@checkid=ajaxgrid_top_1_0_checkFlag").attrs["checked"] = "checked"
            self.tab.ele("#pgcdId").run_js("onCD()")
            self.tab.wait.ele_displayed("传递成功")
            self.tab.wait.ele_deleted("传递成功")
        except Exception as exc:
            self.ctx.log.warning("传递出错：%s", exc)

    def send_job_info(self):
        time.sleep(1)
        self.tab.eles("@text()=发送作业信息")[-1].click()
        time.sleep(5)
        self.tab.ele("@title=是否发起计划").ele("@text()=是").click()
        self.tab.ele("@title=计划周期").ele("@text()=周（日）计划").click()
        time.sleep(2)
        self.tab.ele("#chaxunId").click()
        self.tab.wait.ele_displayed("@text():正在处理。")
        self.tab.wait.ele_deleted("@text():正在处理。")
        self.tab.wait.ele_displayed("项目管理单位：")
        self.fill_plan_time("#img_dateformatjhkssj", self.config.plan_start_time)
        self.fill_plan_time("#img_dateformatjhjssj", self.config.plan_end_time)

        time.sleep(1)
        plan_content = self.config.plan_content_prefix + str(self.tab.ele("#jhnr").value).replace(
            self.config.plan_content_remove_text,
            "",
        )
        self.tab.ele("#jhnr").input(plan_content, clear=True)
        time.sleep(1)
        work_location = self.config.work_location_prefix + self.tab.ele("#gzds").value
        self.tab.ele("#gzds").input(work_location, clear=True)

        self.choose_plan_type()
        self.choose_plan_responsible_person()
        self.choose_plan_worker()

        self.tab.ele("#kcfsdmId").ele(self.config.survey_mode).click()
        self.tab.ele("#gzfzrlxdh").input(self.config.responsible_phone, clear=True)
        self.tab.ele("#ajzylxId").ele(self.config.safety_job_type).click()
        self.tab.ele("#zyejflId").ele(self.config.specialty_category).click()
        self.tab.ele("确认制定计划").click()
        self.tab.wait.ele_displayed("@text():正在处理。")
        self.tab.wait.ele_deleted("@text():正在处理。")
        self.confirm_popup(timeout=None)

    def fill_plan_time(self, selector, time_parts):
        self.tab.ele(selector).click()
        day_color = "red" if self.config.is_weekend else "#4c4c4c"
        self.tab.ele(f"@@id=cellText@@text()={self.config.work_day}@@color={day_color}").click()
        self.tab.ele("#tbSelHour").input(time_parts[0])
        self.tab.ele("#tbSelMin").input(time_parts[1])
        self.tab.ele("#tbSelSec").input(time_parts[2])
        self.tab.ele("#selectButton").click()
        time.sleep(1)

    def choose_plan_type(self):
        self.tab.ele("#inputbuttontest_img").click()
        self.tab.wait.ele_displayed("（旧）低压计量装置装拆")
        time.sleep(1)
        self.tab.ele("@@id=drop_0@@title=作业类型").ele("（旧）低压计量装置装拆").click()
        time.sleep(1)
        self.tab.ele("@@id=drop_1@@title=风险等级").ele("可接受的风险").click()
        time.sleep(1)
        self.tab.ele("@@id=drop_0@@title=作业类型").run_js("retrieve()")
        time.sleep(2)
        self.tab.eles("@@name=numberinner@@text()=6")[0].click.multi()

    def choose_plan_responsible_person(self):
        self.tab.ele("#fzrbsId_img").click()
        self.tab.wait.ele_displayed("@text()=人员名称：")
        self.tab.ele("#rymc").input(self.config.responsible_person)
        self.tab.ele("#rymc").run_js("retrieve()")
        self.tab.ele(f"@@key=5@@text()={self.config.responsible_person}").click()
        self.tab.ele("@text()=手工校验资质").click()
        self.tab.wait.ele_displayed("@text():正在处理。")
        self.tab.wait.ele_deleted("@text():正在处理。")

    def choose_plan_worker(self):
        self.tab.ele("#zyrbsId_img").click()
        self.tab.wait.ele_displayed("@text()=人员名称：")
        self.tab.ele("#rymc").input(self.config.assistant_person)
        self.tab.ele("#rymc").run_js("retrieve()")
        self.tab.ele(f"@@key=5@@text()={self.config.assistant_person}").prev().prev().prev().click()
        self.tab.ele("@text()=手工校验资质").click()
        self.tab.wait.ele_displayed("@text():正在处理。")
        self.tab.wait.ele_deleted("@text():正在处理。")

    def confirm_popup(self, timeout=10):
        if self.tab.wait.ele_displayed("@class=popWindowOKBtnStyle", timeout=timeout):
            self.tab.ele("@class=popWindowOKBtnStyle").click()
