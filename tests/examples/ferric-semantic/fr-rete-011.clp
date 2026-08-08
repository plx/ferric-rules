; FR-RETE-011 primary stage: this definition and activation must be replaced.
(deffacts startup
  (ready))

(defrule replace-me
  (ready)
  =>
  (printout t "old" crlf)
  (assert (result old)))
