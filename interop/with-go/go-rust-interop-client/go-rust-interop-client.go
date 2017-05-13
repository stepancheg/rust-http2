package main

import (
    "log"
    "net/http"
    "golang.org/x/net/http2"
    "crypto/tls"
)

func main() {
    client := http.Client{
        Transport: &http2.Transport{
            AllowHTTP: true,
            TLSClientConfig: &tls.Config{InsecureSkipVerify: true},
        },
        
    }

    resp, err := client.Get("http://localhost:11000/200")
    if err != nil {
        log.Fatal(err)
    }

    if 200 != resp.StatusCode {
        log.Fatal("expecting 200")
    }
}
