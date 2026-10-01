
(deftemplate item (slot value))
(deffacts seed (item (value 30)) (item (value 10)) (item (value 20)))
(defrule probe =>
  (bind ?all (find-all-facts ((?a item) (?b item)) (< ?a:value ?b:value)))
  (printout t "all:" (length$ ?all) ":")
  (progn$ (?entry ?all) (printout t (fact-slot-value ?entry value) ":"))
  (printout t crlf)
  (bind ?first (find-fact ((?a item) (?b item)) (< ?a:value ?b:value)))
  (printout t "first:" (fact-slot-value (nth$ 1 ?first) value) ":"
    (fact-slot-value (nth$ 2 ?first) value) crlf))
