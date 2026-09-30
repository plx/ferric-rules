
(deftemplate item (slot value))
(deffacts seed (item (value 10)) (padding) (item (value 20)))
(deffunction through (?address) (fact-index ?address))
(defrule probe
  ?f <- (item (value 10))
  ?g <- (item (value 20))
  =>
  (bind ?copy ?f)
  (printout t "alias:" (fact-index ?copy) crlf)
  (if (fact-existp ?f) then (printout t "if:" (fact-index ?f) crlf))
  (loop-for-count (?i (fact-index ?g) (fact-index ?g)) do
    (printout t "loop:" ?i crlf))
  (switch (fact-index ?g)
    (case (fact-index ?g) then (printout t "switch:" (fact-index ?g) crlf)))
  (progn$ (?index (create$ (fact-index ?g)))
    (printout t "progn:" ?index crlf))
  (do-for-fact ((?query item)) (= (fact-index ?query) (fact-index ?g))
    (printout t "query:" (fact-index ?g) crlf))
  (printout t "function:" (through ?g) crlf)
  (while (fact-existp ?f) do
    (printout t "while:" (fact-index ?f) crlf)
    (retract ?f))
  (printout t "stale:" (fact-existp ?copy) ":" (fact-index ?copy) crlf))
