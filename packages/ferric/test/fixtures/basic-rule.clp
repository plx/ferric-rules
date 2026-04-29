;; Basic rule for smoke testing.
;; Fires once when initial-fact is present.
(defrule hello-world
  (initial-fact)
  =>
  (printout t "hello from ferric" crlf))
