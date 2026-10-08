;; Incomplete exponents are ordinary symbols in fact fields and literal constraints.
;; Level: boundary
;; Covers: source-scanner, deffacts, ordered-facts, explode$
(deffacts seed (tokens 5e 1.5e 1e+ 1e- +1e -1e+ .5e -.5e-))
(defrule match
  (tokens 5e 1.5e 1e+ 1e- +1e -1e+ .5e -.5e-)
  (tokens $?values)
  => (printout t (length$ ?values) " " ?values crlf)
  (printout t (explode$ "5e 1.5e 1e+ 1e- +1e -1e+ .5e -.5e-") crlf))
