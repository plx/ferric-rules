; format writes to its logical name and returns the formatted string.
; Use nil to return text without writing, for example inside printout.
(deffacts startup (run-format))

(defrule do-format
    (run-format)
    =>
    (format t "num=%d%n" 42)
    (printout t (format nil "str=%s" "hello") crlf)
    (printout t (format nil "flt=%.1f" 3.5) crlf))
