package agents

import "errors"

// Bridge: the relay is perl on a Unix socket, so Windows has no bridged servers.
func Bridge(name, server string, run Bridged) []McpServer { return nil }

func served(string) []McpServer { return nil }

// BrowserStop: nothing listens on Windows.
func BrowserStop() {}

// BrowserListen: there is no socket on Windows.
func BrowserListen() (string, error) { return "", errors.New("the agent browser's socket is Unix's") }
