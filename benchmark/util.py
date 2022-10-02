import json, subprocess, os, sys, tempfile


class Profile:
    """
    read profile given filename; if provided `skip_begin_profiles`, then it will skip such number of profiles in the beginning,
    by default to 5 because usually the first few profiles are not stable yet
    """
    def __init__(self, filename, skip_begin_profiles=5):
        assert isinstance(filename, str)
        with open(filename, "r", encoding="utf8") as f:
            lines = f.readlines()
        self.partition_config = None
        self.entries = []
        skipped = 0
        for line_idx, line in enumerate(lines):
            line = line.strip("\r\n ")
            if line == "":
                break
            value = json.loads(line)
            if line_idx == 0:
                self.partition_config = PartitionConfig.from_json(value)
            elif line_idx == 1:
                self.benchmark_config = value
            else:
                if skipped < skip_begin_profiles:
                    skipped += 1
                else:
                    self.entries.append(value)
    def __repr__(self):
        return f"Profile {{ partition_config: {self.partition_config}, entries: [...{len(self.entries)}] }}"
    def sum_decoding_time(self):
        decoding_time = 0
        for entry in self.entries:
            decoding_time += entry["decoding_time"]
        return decoding_time
    def average_decoding_time(self):
        return self.sum_decoding_time() / len(self.entries)
    def sum_syndrome_num(self):
        syndrome_num = 0
        for entry in self.entries:
            syndrome_num += entry["syndrome_num"]
        return syndrome_num
    def average_decoding_time_per_syndrome(self):
        return self.sum_decoding_time() / self.sum_syndrome_num()
    def sum_computation_cpu_seconds(self):
        total_computation_cpu_seconds = 0
        for entry in self.entries:
            computation_cpu_seconds = 0
            for event_time in entry["solver_profile"]["primal"]["event_time_vec"]:
                computation_cpu_seconds += event_time["end"] - event_time["start"]
            total_computation_cpu_seconds += computation_cpu_seconds
        return total_computation_cpu_seconds
    def average_computation_cpu_seconds(self):
        return self.sum_computation_cpu_seconds() / len(self.entries)

class VertexRange:
    def __init__(self, start, end):
        self.range = (start, end)
    def __repr__(self):
        return f"[{self.range[0]}, {self.range[1]}]"

class PartitionConfig:
    def __init__(self, vertex_num):
        self.vertex_num = vertex_num
        self.partitions = [VertexRange(0, vertex_num)]
        self.fusions = []
    def __repr__(self):
        return f"PartitionConfig {{ vertex_num: {self.vertex_num}, partitions: {self.partitions}, fusions: {self.fusions} }}"
    @staticmethod
    def from_json(value):
        vertex_num = value['vertex_num']
        config = PartitionConfig(vertex_num)
        config.partitions.clear()
        for range in value['partitions']:
            config.partitions.append(VertexRange(range[0], range[1]))
        for pair in value['fusions']:
            config.fusions.append((pair[0], pair[1]))
        return config

git_root_dir = subprocess.run("git rev-parse --show-toplevel", cwd=os.path.dirname(os.path.abspath(__file__))
    , shell=True, check=True, capture_output=True).stdout.decode(sys.stdout.encoding).strip(" \r\n")
rust_dir = git_root_dir

FUSION_BLOSSOM_COMPILATION_DONE = False
if 'MANUALLY_COMPILE_QEC' in os.environ and os.environ["MANUALLY_COMPILE_QEC"] == "TRUE":
    FUSION_BLOSSOM_COMPILATION_DONE = True
FUSION_BLOSSOM_ENABLE_UNSAFE_POINTER = False
if 'FUSION_BLOSSOM_ENABLE_UNSAFE_POINTER' in os.environ and os.environ["FUSION_BLOSSOM_ENABLE_UNSAFE_POINTER"] == "TRUE":
    FUSION_BLOSSOM_ENABLE_UNSAFE_POINTER = True
def compile_code_if_necessary(additional_build_parameters=None):
    global FUSION_BLOSSOM_COMPILATION_DONE
    if FUSION_BLOSSOM_COMPILATION_DONE is False:
        build_parameters = ["cargo", "build", "--release"]
        if FUSION_BLOSSOM_ENABLE_UNSAFE_POINTER:
            build_parameters += ["--features", "unsafe_pointer"]
        if additional_build_parameters is not None:
            build_parameters += additional_build_parameters
        # print(build_parameters)
        process = subprocess.Popen(build_parameters, universal_newlines=True, stdout=sys.stdout, stderr=sys.stderr, cwd=rust_dir)
        process.wait()
        assert process.returncode == 0, "compile has error"
        FUSION_BLOSSOM_COMPILATION_DONE = True

def fusion_blossom_benchmark_command(d=None, p=None, total_rounds=None, r=None, noisy_measurements=None, n=None):
    assert d is not None
    assert p is not None
    fusion_path = os.path.join(rust_dir, "target", "release", "fusion_blossom")
    command = [fusion_path, "benchmark", f"{d}", f"{p}"]
    if total_rounds is not None:
        command += ["-r", f"{total_rounds}"]
    elif r is not None:
        command += ["-r", f"{r}"]
    if noisy_measurements is not None:
        command += ["-n", f"{noisy_measurements}"]
    elif n is not None:
        command += ["-n", f"{n}"]
    return command

def run_command_get_stdout(command, no_stdout=False, use_tmp_out=False, stderr_to_stdout=False):
    compile_code_if_necessary()
    env = os.environ.copy()
    env["RUST_BACKTRACE"] = "full"
    stdout = subprocess.PIPE
    if use_tmp_out:
        out_file = tempfile.NamedTemporaryFile(delete=False)
        out_filename = out_file.name
        stdout = out_file
    if no_stdout:
        stdout = sys.stdout
    process = subprocess.Popen(command, universal_newlines=True, env=env, stdout=stdout, stderr=(stdout if stderr_to_stdout else sys.stderr), bufsize=100000000)
    stdout, _ = process.communicate()
    if use_tmp_out:
        out_file.flush()
        out_file.close()
        with open(out_filename, "r", encoding="utf8") as f:
            stdout = f.read()
        os.remove(out_filename)
    return stdout, process.returncode
