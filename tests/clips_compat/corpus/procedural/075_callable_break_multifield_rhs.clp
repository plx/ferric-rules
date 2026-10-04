(defrule run =>
  (foreach ?x (create$ a b c)
    (if (eq ?x b) then (break))
    (printout t "foreach:" ?x crlf))
  (progn$ (?x (create$ a b c))
    (if (eq ?x b) then (break))
    (printout t "progn$:" ?x crlf))
  (printout t "after" crlf))
