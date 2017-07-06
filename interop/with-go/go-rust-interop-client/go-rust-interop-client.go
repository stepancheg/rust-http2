package main

import (
    "log"
    "net/http"
    "golang.org/x/net/http2"
    "crypto/tls"
    "os"
    "fmt"
    "io"
)

func main() {
    client := http.Client{
        Transport: &http2.Transport{
            AllowHTTP: false,
            TLSClientConfig: &tls.Config{InsecureSkipVerify: true},
        },
    }

    if len(os.Args) != 2 {
        log.Fatalf("expecting exactly one arg, got: %d", len(os.Args) - 1);
    }

    if os.Args[1] == "200" {

        resp, err := client.Get("https://localhost:11000/200")
        if err != nil {
            log.Fatal(err)
        }

        if 200 != resp.StatusCode {
            log.Fatal("expecting 200")
        }
    } else if os.Args[1] == "long" {
        bs := 100000
        count := 10000

        resp, err := client.Get(fmt.Sprintf("https://localhost:11000/blocks/%d/%d", bs, count))
        if err != nil {
            log.Fatal(err)
        }

        exp := bs * count

        if bs * count != exp {
            log.Fatalf("xxxx %d", bs * count)
        }

        if 200 != resp.StatusCode {
            log.Fatal("expecting 200")
        }

        total := 0

        for {
            b := make([]byte, 44000);
            count, err := resp.Body.Read(b);
            total += count

            if err == io.EOF {
                if total != exp {
                    log.Fatalf("expecting %d got %d", exp, total)
                }
                break
            } else if err != nil {
                log.Fatalf("couldn't read after reading %d: %s", total, err)
            }
        }
    } else {
        log.Fatalf("unknown mode: %s", os.Args[1]);
    }
}
