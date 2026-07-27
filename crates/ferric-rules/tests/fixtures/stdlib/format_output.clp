; format function: printf-style string formatting, result printed via printout.
; In Ferric, format returns a string and does not write to the router directly.
; Use format with nil channel and capture the result, or inline in printout.
(deffacts startup (run-format))

(defrule do-format
    (run-format)
    =>
    (printout t (format nil "num=%d" 42) crlf)
    (printout t (format nil "str=%s" "hello") crlf)
    (printout t (format nil "flt=%.1f" 3.5) crlf))
