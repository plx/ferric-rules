(deffacts seed (item))
(defrule run ?f <- (item) => (printout t "before " (str-cat ?f) "unexpected" crlf))
