(defrule echo-line
    (prompt-line)
    =>
    (printout t (format nil "n=%d" 42) crlf)
    (bind ?line (readline))
    (printout t "got: " ?line crlf))
