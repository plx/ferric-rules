(deffacts d (n 1) (n -1) (n 2))
(defrule a (n ?x&:(if (> ?x 0) then TRUE else FALSE)) => (printout t "if=" ?x crlf))
(defrule b (n ?x) (test (switch ?x (case 1 then TRUE) (default FALSE))) => (printout t "switch=" ?x crlf))
(defrule c (n ?x&:(switch ?x (case 2 then TRUE) (default FALSE))) => (printout t "predicate=" ?x crlf))
