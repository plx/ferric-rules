(defrule divide-by-zero
    (begin)
    =>
    (printout t (/ 1 0) crlf))
