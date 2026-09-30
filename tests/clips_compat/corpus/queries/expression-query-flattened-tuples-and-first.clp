
(deftemplate left-item (slot value))
(deftemplate right-item (slot value))
(deffacts seed
  (right-item (value x)) (left-item (value a))
  (right-item (value y)) (left-item (value b)))
(defrule probe =>
  (bind ?all (find-all-facts ((?a left-item) (?b right-item)) TRUE))
  (printout t "all:" (length$ ?all) ":")
  (progn$ (?entry ?all) (printout t (fact-slot-value ?entry value) ":"))
  (printout t crlf)
  (bind ?first (find-fact ((?a left-item) (?b right-item)) TRUE))
  (printout t "first:" (length$ ?first) ":"
    (fact-slot-value (nth$ 1 ?first) value) ":"
    (fact-slot-value (nth$ 2 ?first) value) crlf)
  (bind ?reverse (find-all-facts ((?b right-item) (?a left-item)) TRUE))
  (printout t "reverse:" (length$ ?reverse) ":")
  (progn$ (?entry ?reverse) (printout t (fact-slot-value ?entry value) ":"))
  (printout t crlf))
