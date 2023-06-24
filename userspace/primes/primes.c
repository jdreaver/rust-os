extern void syscall_print(char *str, int len);
extern void syscall_exit(int exit_code);

int is_prime(int x) {
	for (int i = 2; i < x; i++) {
		if (x % i == 0) {
			return 0;
		}
	}
	return 1;
}

int naive_nth_prime(int n) {
	int i = 2;
	int count = 0;
	while (count < n) {
		if (is_prime(i)) {
			count++;
		}
		i++;
	}
	return i;
}

int _strlen(char *str) {
	int len = 0;
	while (*str != '\0') {
		len++;
		str++;
	}
	return len;
}

void int_to_str(int x, char buffer[]) {
	// First dump the number into the buffer in reverse order
	int i = 0;
	while (x > 0) {
		buffer[i] = (x % 10) + '0';
		x /= 10;
		i++;
	}
	buffer[i] = '\0';

	// Now reverse the buffer
	int j = i - 1;
	i = 0;
	while (i < j) {
		char tmp = buffer[i];
		buffer[i] = buffer[j];
		buffer[j] = tmp;
		i++;
		j--;
	}
}

void _start() {
	int nth_prime = naive_nth_prime(1000);

	char buffer[100] = "The 1000th prime is: ";
	char *ptr = buffer + _strlen(buffer);
	int_to_str(nth_prime, ptr);

	syscall_print((char *)buffer, _strlen(buffer));
	syscall_exit(0);
}
