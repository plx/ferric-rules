
;; Infinity signs are portable. NaN signs from libm/printf are not; direct
;; printout of both explicit NaN signs is covered by Rust host-value tests.
(defrule exercise =>
(printout t (create$ 1.0e309 -1.0e309) crlf)
(printout t 1.0e309 "|" -1.0e309 crlf)
)
