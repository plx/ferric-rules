(deffunction choose (?x)
  (bind ?answer 0)
  (switch ?x
    (case 1 then (bind ?answer 10))
    (default (bind ?answer 20)))
  (+ ?answer 1))
(defrule probe => (printout t (choose 1) ":" (choose 2) crlf))
