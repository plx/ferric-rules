;; Protocol: load these constructs, reset, load the companion assertions, then run.
;; Top-level assert must preserve singleton, multiple, empty, and defaulted multislots.
(deftemplate bag
  (slot id)
  (multislot left (default seed))
  (multislot right))
(defrule exact-fields
  (declare (salience 10))
  (bag (id several) (right ?first ?last))
  => (printout t "two:" ?first ":" ?last crlf))
(defrule observe
  (bag (id ?id) (left $?left) (right $?right))
  => (printout t ?id " " ?left "|" ?right crlf))
