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

char *_strcpy(char *dest, char *src) {
	int len = 0;
	while (*src != '\0') {
		*dest = *src;
		len++;
		src++;
		dest++;
	}
	*dest = '\0';
	return dest;
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

int str_to_int(char *str) {
	int x = 0;
	while (*str != '\0') {
		x *= 10;

		if (*str < '0' || *str > '9') {
			char *err = "Error: non-digit character in integer string\n";
			syscall_print(err, _strlen(err));
			syscall_exit(1);
		}

		x += *str - '0';
		str++;
	}
	return x;
}

int main(int argc, char *argv[]) {
	// First argument is the index of the prime to find
	if (argc != 2) {
		char *usage = "Usage: primes <n>\n";
		syscall_print(usage, _strlen(usage));
		syscall_exit(1);
	}
	char *n_str = argv[1];
	int n = str_to_int(n_str);

	// Perform computation
	int nth_prime = naive_nth_prime(n);

	// Construct output message
	char buffer[100] = "The ";
	char *ptr = buffer + _strlen(buffer);
	ptr = _strcpy(ptr, n_str);
	ptr = _strcpy(ptr, "th prime is: ");
	int_to_str(nth_prime, ptr);

	syscall_print((char *)buffer, _strlen(buffer));

	return 0;
}
