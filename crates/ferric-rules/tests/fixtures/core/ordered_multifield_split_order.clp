;; One rule and one fact isolate capture values and partition firing order.
(deffacts input (row a marker b marker c))
(defrule observe
  (row $?left marker $?right)
  => (printout t (length$ ?left) ":" (length$ ?right) " " ?left "|" ?right crlf))
