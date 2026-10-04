(deftemplate item (slot v))
(defrule run =>
  (printout t "exists:[" (fact-existp -1) "] relation:[" (fact-relation -1) "]" crlf)
  (printout t "names:[" (fact-slot-names -1) "] value:[" (fact-slot-value -1 v) "]" crlf)
  (retract -1)
  (printout t "continued" crlf))
