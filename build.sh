cd $(dirname $0)
sh ./packages/jade-data/build.sh
node --experimental-strip-types ./scripts/regen.ts