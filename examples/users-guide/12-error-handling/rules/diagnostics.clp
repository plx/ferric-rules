(defrule focus-missing
    (begin)
    =>
    (focus DOES-NOT-EXIST)
    (printout t "tried to focus a missing module" crlf))
