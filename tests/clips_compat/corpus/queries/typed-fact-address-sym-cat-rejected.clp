(deffacts seed (item))
(defrule run ?f <- (item) => (printout t "before " (sym-cat ?f) "unexpected" crlf))
