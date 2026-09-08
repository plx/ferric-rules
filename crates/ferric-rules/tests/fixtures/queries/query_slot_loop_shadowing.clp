;; Ordinary loop bindings mask ?f, while compact ?f:value stays query-scoped.
(deftemplate item (slot value) (multislot parts))
(deffacts seed (item (value 10) (parts a b)))
(defrule probe =>
  (bind ?f 99)
  (bind ?f-index 88)
  (do-for-all-facts ((?f item)) TRUE
    (loop-for-count (?f 1 2)
      (printout t "loop:" ?f ":" ?f:value crlf))
    (progn$ (?f ?f:parts)
      (printout t "progn:" ?f ":" ?f-index ":" ?f:value crlf))
    (printout t "query:" ?f:value crlf))
  (printout t "local:" ?f ":" ?f-index crlf))
