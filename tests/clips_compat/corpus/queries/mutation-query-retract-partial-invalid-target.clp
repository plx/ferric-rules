;; retract commits each target before evaluating the next one, so a later
;; invalid target still leaves the first fact retracted.
(deftemplate item (slot kind))
(deffacts seed (item (kind first)) (item (kind second)))
(deffunction invalid-target (?first ?second)
  (printout t "first:" (fact-existp ?first) ":second:" (fact-existp ?second) crlf)
  wrong)
(defrule controller
  ?first <- (item (kind first))
  ?second <- (item (kind second))
  =>
  (retract ?first (invalid-target ?first ?second))
  (printout t "after-invalid" crlf))
