(deffunction f (?x) (eval "?x"))
(defrule run => (printout t "prefix:" (f 7) "after" crlf))
