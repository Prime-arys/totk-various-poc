#---------------------------------------------------------------------------------
# skyline-totk build rules (derived from Skyline's nso.mk, itself derived from
# devkitPro's switch_rules).
#
# The output is an NSO meant to be dropped in the game's exefs as `subsdk9`.
# LINKERSCRIPTS holds the layout script plus a (currently empty) symbol script
# which can be used to hardcode addresses of game functions if ever needed.
#---------------------------------------------------------------------------------

LINKERSCRIPTS    := linkerscripts

#---------------------------------------------------------------------------------
#.SUFFIXES:
#---------------------------------------------------------------------------------

ifeq ($(strip $(DEVKITPRO)),)
$(error "Please set DEVKITPRO in your environment. export DEVKITPRO=<path to>/devkitpro")
endif

TOPDIR ?= $(CURDIR)
include $(DEVKITPRO)/libnx/switch_rules

#---------------------------------------------------------------------------------
# TARGET is the name of the output
# BUILD is the directory where object files & intermediate files will be placed
# SOURCES is a list of directories containing source code
# DATA is a list of directories containing data files
# INCLUDES is a list of directories containing header files
#---------------------------------------------------------------------------------

# NOTE: TARGET and BUILD are now passed from parent Makefile
TARGET		?=	$(notdir $(CURDIR))
BUILD		?=	build
SOURCES		:= 	source $(filter-out %.c %.cpp %.s,$(wildcard source/* source/*/* source/*/*/* source/*/*/*/*))
DATA		:=	data
INCLUDES	:=	include libs/libeiffel/include

#---------------------------------------------------------------------------------
# options for code generation
#---------------------------------------------------------------------------------
ARCH	:=	-march=armv8-a -mtune=cortex-a57 -mtp=soft -fPIC -ftls-model=local-exec

CFLAGS	:=	-g -Wall -ffunction-sections \
			$(ARCH) $(DEFINES)

CFLAGS	+=	$(INCLUDE) -D__SWITCH__

ifneq ($(strip $(NOLOG)),)
CFLAGS	+=	  "-DNOLOG"
endif

CXXFLAGS	:= $(CFLAGS) -fno-rtti -fomit-frame-pointer -fno-exceptions -fno-asynchronous-unwind-tables -fno-unwind-tables -enable-libstdcxx-allocator=new -fpermissive 

ASFLAGS	:=	-g $(ARCH)
# The two linker scripts used to be named inside switch.specs, relative to the
# build directory ("../linkerscripts/..."), which only worked while that
# directory sat right next to the sources. They are passed from here instead,
# so the build can happen anywhere (the repository builds into output/skyline).
LINKER_SCRIPTS = -Wl,-T,$(TOPDIR)/linkerscripts/application.ld \
                 -Wl,$(TOPDIR)/linkerscripts/syms.ld
LDFLAGS  =  -specs=$(TOPDIR)/switch.specs $(LINKER_SCRIPTS) -g $(ARCH) -Wl,-Map,$(notdir $*.map) -Wl,--version-script=$(TOPDIR)/exported.txt -Wl,-init=__custom_init -Wl,-fini=__custom_fini -Wl,--export-dynamic -nodefaultlibs


LIBS	:= -lgcc -lstdc++ -u malloc

#---------------------------------------------------------------------------------
# list of directories containing libraries, this must be the top level containing
# include and lib
#---------------------------------------------------------------------------------
LIBDIRS	:= $(PORTLIBS) $(LIBNX)

#---------------------------------------------------------------------------------
# no real need to edit anything past this point unless you need to add additional
# rules for different file extensions
#---------------------------------------------------------------------------------
# Which half of this file runs: the top one, from the source directory, then
# the bottom one after make has been re-run inside BUILD. devkitPro tells them
# apart by comparing BUILD with the name of the current directory, which only
# holds when BUILD sits right next to the sources. Here it can be anywhere (the
# repository builds into output/skyline), so the second pass says who it is.
ifneq ($(IN_BUILD_DIR),1)
#---------------------------------------------------------------------------------

export OUTPUT	?=	$(CURDIR)/$(TARGET)
export TOPDIR	:=	$(CURDIR)

export VPATH	:=	$(foreach dir,$(SOURCES),$(CURDIR)/$(dir)) \
			$(foreach dir,$(DATA),$(CURDIR)/$(dir))

export DEPSDIR	?=	$(abspath $(BUILD))

CFILES		:=	$(foreach dir,$(SOURCES),$(notdir $(wildcard $(dir)/*.c)))
CPPFILES	:=	$(foreach dir,$(SOURCES),$(notdir $(wildcard $(dir)/*.cpp)))
SFILES		:=	$(foreach dir,$(SOURCES),$(notdir $(wildcard $(dir)/*.s)))

#---------------------------------------------------------------------------------
# use CXX for linking C++ projects, CC for standard C
#---------------------------------------------------------------------------------
ifeq ($(strip $(CPPFILES)),)
#---------------------------------------------------------------------------------
	export LD	:=	$(CC)
#---------------------------------------------------------------------------------
else
#---------------------------------------------------------------------------------
	export LD	:=	$(CXX)
#---------------------------------------------------------------------------------
endif
#---------------------------------------------------------------------------------
export OFILES_BIN	:=	$(addsuffix .o,$(BINFILES))
export OFILES_SRC	:=	$(CPPFILES:.cpp=.o) $(CFILES:.c=.o) $(SFILES:.s=.o)
export OFILES 	:=	$(OFILES_BIN) $(OFILES_SRC)
export HFILES_BIN	:=	$(addsuffix .h,$(subst .,_,$(BINFILES)))

export INCLUDE	:=	$(foreach dir,$(INCLUDES),-I$(CURDIR)/$(dir)) \
			$(foreach dir,$(LIBDIRS),-I$(dir)/include) \
			-I$(abspath $(BUILD))

export LIBPATHS	:=	$(foreach dir,$(LIBDIRS),-L$(dir)/lib)

ifeq ($(strip $(ICON)),)
	icons := $(wildcard *.jpg)
	ifneq (,$(findstring $(TARGET).jpg,$(icons)))
		export APP_ICON := $(TOPDIR)/$(TARGET).jpg
	else
		ifneq (,$(findstring icon.jpg,$(icons)))
			export APP_ICON := $(TOPDIR)/icon.jpg
		endif
	endif
else
	export APP_ICON := $(TOPDIR)/$(ICON)
endif

ifeq ($(strip $(NO_ICON)),)
	export NROFLAGS += --icon=$(APP_ICON)
endif

ifeq ($(strip $(NO_NACP)),)
	export NROFLAGS += --nacp=$(CURDIR)/$(TARGET).nacp
endif

.PHONY: $(BUILD) clean all

#---------------------------------------------------------------------------------

all: $(BUILD)

$(BUILD):
	@[ -d $@ ] || mkdir -p $@
	$(MAKE) -C $(BUILD) -f $(CURDIR)/$(MAKE_NSO) IN_BUILD_DIR=1

#---------------------------------------------------------------------------------
clean:
	@echo clean ...
	@rm -fr $(BUILD) $(OUTPUT).nso $(OUTPUT).elf $(OUTPUT).map


#---------------------------------------------------------------------------------
else
.PHONY:	all

DEPENDS	:=	$(OFILES:.o=.d)

#---------------------------------------------------------------------------------
# main targets
#---------------------------------------------------------------------------------
%.nso: %.elf
	@elf2nso $< $@
	@echo built ... $(notdir $@)

%.nro: %.elf
	@elf2nro $< $@ $(NROFLAGS)
	@echo built ... $(notdir $@)

#---------------------------------------------------------------------------------
all	: $(OUTPUT).nso

$(OUTPUT).elf   : $(OFILES)


$(OFILES_SRC)	: $(HFILES_BIN)

#---------------------------------------------------------------------------------
# you need a rule like this for each extension you use as binary data
#---------------------------------------------------------------------------------
%.bin.o	%_bin.h :	%.bin
#---------------------------------------------------------------------------------
	@echo $(notdir $<)
	@$(bin2o)

-include $(DEPENDS)

#---------------------------------------------------------------------------------------
endif
#---------------------------------------------------------------------------------------
