;; Phase 4: I/O and environment function surface fixture.
;; Exercises printout, format, get-focus, get-focus-stack.

(defrule test-io
    (go)
    =>
    (printout t "hello" tab "world" crlf)
    (printout t (format nil "x=%d y=%f" 10 2.5) crlf)
    (printout t "focus: " (get-focus) crlf)
    (printout t "stack: " (get-focus-stack) crlf))

(deffacts startup (go))
